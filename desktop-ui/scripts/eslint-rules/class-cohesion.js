/**
 * er/class-cohesion — flag a class that is several classes in one (LCOM4).
 *
 * A class's members (methods, fields, accessors) are nodes. Two members are
 * joined when one uses the other through `this` (`this.x`, `this.#x`,
 * `this.method()`). The connected groups are the class's responsibilities: a
 * group never touches another group's state or methods, so it could be its own
 * class. The constructor is left out because it initialises everything, which
 * would join every group; that is the standard LCOM4 convention.
 *
 * Classes that `extend` another class or `implement` an interface are skipped:
 * their members answer to that contract and often share state through
 * inherited members this rule can't see (a storage port's methods are each an
 * independent query, by design).
 *
 * A group counts once its members span `minGroupLines` lines, so a class with
 * a few small independent getters is left alone. The rule reports when a class
 * has more than `maxGroups` groups that size and names each group's biggest
 * members: that list is the split.
 */

function memberName(member) {
  const key = member.key;
  if (!key) return null;
  if (key.type === "PrivateIdentifier") return `#${key.name}`;
  if (key.type === "Identifier" && !member.computed) return key.name;
  if (key.type === "Literal") return String(key.value);
  return null;
}

const CLASS_NODES = new Set(["ClassBody", "ClassDeclaration", "ClassExpression"]);
const SKIP_KEYS = new Set(["parent", "loc", "range"]);

/** A nested class, or a plain function (which rebinds `this`), starts a new `this`. */
function startsNewThis(node) {
  if (CLASS_NODES.has(node.type)) return true;
  const isFunction = node.type === "FunctionDeclaration" || node.type === "FunctionExpression";
  return isFunction && node.parent?.type !== "MethodDefinition";
}

/** The member a `this.x` / `this.#x` expression names, if it is one. */
function thisMemberName(node) {
  if (node.type !== "MemberExpression" || node.object.type !== "ThisExpression" || node.computed) {
    return null;
  }
  const p = node.property;
  if (p.type === "PrivateIdentifier") return `#${p.name}`;
  return p.type === "Identifier" ? p.name : null;
}

function childNodes(node) {
  const out = [];
  for (const [key, value] of Object.entries(node)) {
    if (SKIP_KEYS.has(key)) continue;
    if (Array.isArray(value)) out.push(...value.filter((v) => typeof v?.type === "string"));
    else if (typeof value?.type === "string") out.push(value);
  }
  return out;
}

/** Member names this subtree reaches through `this`, not entering a new `this` scope. */
function thisUses(node, out = new Set()) {
  if (startsNewThis(node)) return out;
  const name = thisMemberName(node);
  if (name) out.add(name);
  for (const child of childNodes(node)) thisUses(child, out);
  return out;
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
      description: "Flag classes whose members fall into groups that never use each other (LCOM4).",
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
    const { maxGroups = 1, minGroupLines = 40 } = context.options[0] ?? {};

    function check(classNode) {
      if (classNode.superClass || classNode.implements?.length) return;
      const members = classNode.body.body
        .filter((m) => !(m.type === "MethodDefinition" && m.kind === "constructor"))
        .filter((m) => m.type !== "StaticBlock" && !m.static)
        .map((m) => ({ node: m, name: memberName(m) }))
        .filter((m) => m.name !== null);
      if (members.length <= maxGroups) return;

      // Getter/setter pairs share a name: treat them as one member.
      const index = new Map();
      for (const m of members) if (!index.has(m.name)) index.set(m.name, index.size);
      const uf = makeUnionFind(index.size);
      for (const m of members) {
        const from = index.get(m.name);
        for (const used of thisUses(m.node.value ?? m.node)) {
          const to = index.get(used);
          if (to !== undefined) uf.union(from, to);
        }
      }

      const groups = new Map();
      for (const m of members) {
        const root = uf.find(index.get(m.name));
        const group = groups.get(root) ?? { lines: 0, members: [] };
        const lines = m.node.loc.end.line - m.node.loc.start.line + 1;
        group.lines += lines;
        group.members.push({ name: m.name, lines });
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
          .map((m) => m.name)
          .join(", ") + (g.members.length > 3 ? ", …" : "");
      const name = classNode.id?.name ?? "This class";
      context.report({
        node: classNode.id ?? classNode,
        message: `${name} holds ${substantial.length} groups of members that never use each other: ${substantial
          .map((g) => `[${describe(g)}]`)
          .join(" · ")}. Each group could be its own class.`,
      });
    }

    return { ClassDeclaration: check, ClassExpression: check };
  },
};
