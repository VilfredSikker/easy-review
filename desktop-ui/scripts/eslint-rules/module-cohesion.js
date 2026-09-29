/**
 * er/module-cohesion — flag a module that holds several unrelated jobs.
 *
 * Size is a poor proxy for "too much in one file": a long module whose parts
 * all work together is fine, and splitting it only scatters one idea across
 * files. What makes a module hard to read is unrelated responsibilities
 * sharing a file. This rule measures that directly.
 *
 * Every top-level value declaration (function, class, const/let) is a node.
 * Two nodes are joined when one references the other. The connected groups
 * are the module's responsibilities: code in one group never touches code in
 * another, so each group could live in its own file unchanged. Types and
 * interfaces are left out (they describe data, and often span groups), and so
 * are imports (two groups using the same library aren't related by it).
 *
 * A group counts once its declarations span `minGroupLines` lines, so a module
 * of many small helpers (a formatters file) is left alone. The rule reports
 * when a module has more than `maxGroups` groups that size, and names each
 * group's biggest declarations: that list is the split.
 *
 * In a `.svelte` file only the instance script is analysed: the markup uses
 * everything, which would join every group, while script state that splits
 * into independent groups is exactly what a subcomponent should own.
 */

const VALUE_DECL = new Set(["FunctionDeclaration", "ClassDeclaration", "VariableDeclaration"]);

/** The declaration statement a top-level node stands for (unwraps `export`). */
function declOf(statement) {
  if (
    (statement.type === "ExportNamedDeclaration" ||
      statement.type === "ExportDefaultDeclaration") &&
    statement.declaration
  ) {
    return statement.declaration;
  }
  return statement;
}

/** Top-level statements, including those inside a Svelte instance `<script>`. */
function topLevelStatements(program) {
  const out = [];
  for (const node of program.body) {
    if (node.type === "SvelteScriptElement") {
      const isModule = node.startTag?.attributes?.some(
        (a) => a.key?.name === "context" || a.key?.name === "module",
      );
      if (!isModule) out.push(...(node.body ?? []));
    } else {
      out.push(node);
    }
  }
  return out;
}

function namesOf(decl) {
  if (decl.type === "VariableDeclaration") {
    return decl.declarations.flatMap((d) => (d.id.type === "Identifier" ? [d.id.name] : []));
  }
  return decl.id ? [decl.id.name] : [];
}

function makeUnionFind(n) {
  const parent = Array.from({ length: n }, (_, i) => i);
  const find = (i) => (parent[i] === i ? i : (parent[i] = find(parent[i])));
  return { find, union: (a, b) => (parent[find(a)] = find(b)) };
}

export default {
  meta: {
    type: "suggestion",
    docs: {
      description:
        "Flag modules whose top-level code falls into several groups that never reference each other.",
    },
    schema: [
      {
        type: "object",
        properties: {
          maxGroups: { type: "integer", minimum: 1 },
          minGroupLines: { type: "integer", minimum: 1 },
        },
        additionalProperties: false,
      },
    ],
  },
  create(context) {
    const { maxGroups = 2, minGroupLines = 60 } = context.options[0] ?? {};
    const source = context.sourceCode;

    return {
      "Program:exit"(program) {
        const decls = topLevelStatements(program)
          .map(declOf)
          .filter((d) => VALUE_DECL.has(d.type) && namesOf(d).length > 0);
        if (decls.length <= maxGroups) return;

        const indexOfNode = new Map(decls.map((d, i) => [d, i]));
        const uf = makeUnionFind(decls.length);

        // Which top-level declaration contains a given node, if any.
        const owner = (node) => {
          for (let n = node; n; n = n.parent) {
            const decl = indexOfNode.get(n) ?? indexOfNode.get(n.declaration);
            if (decl !== undefined) return decl;
          }
          return undefined;
        };

        for (const decl of decls) {
          const from = indexOfNode.get(decl);
          for (const variable of source.getDeclaredVariables(decl)) {
            for (const ref of variable.references) {
              const user = owner(ref.identifier);
              if (user !== undefined && user !== from) uf.union(user, from);
            }
          }
        }

        const groups = new Map();
        for (const [i, decl] of decls.entries()) {
          const root = uf.find(i);
          const group = groups.get(root) ?? { lines: 0, members: [] };
          const lines = decl.loc.end.line - decl.loc.start.line + 1;
          group.lines += lines;
          group.members.push({ names: namesOf(decl), lines });
          groups.set(root, group);
        }

        const substantial = [...groups.values()]
          .filter((g) => g.lines >= minGroupLines)
          .sort((a, b) => b.lines - a.lines);
        if (substantial.length <= maxGroups) return;

        const describe = (g) =>
          g.members
            .sort((a, b) => b.lines - a.lines)
            .slice(0, 3)
            .flatMap((m) => m.names)
            .slice(0, 3)
            .join(", ") + (g.members.length > 3 ? ", …" : "");
        const groupsText = substantial.map((g) => `[${describe(g)}]`).join(" · ");
        const isComponent = context.filename.endsWith(".svelte");
        context.report({
          loc: { line: 1, column: 0 },
          message: isComponent
            ? `This component's script holds ${substantial.length} unrelated groups of state and logic that never reference each other: ${groupsText}. Each could be a subcomponent that owns its group.`
            : `This module holds ${substantial.length} unrelated groups of code that never reference each other: ${groupsText}. Give each group its own module.`,
        });
      },
    };
  },
};
