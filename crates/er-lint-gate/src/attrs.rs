//! Attribute predicates shared by the gate's source checks.

/// Whether an item carries `#[cfg(test)]`, which keeps test-only code out of
/// every rule.
pub(crate) fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg")
            && a.meta
                .require_list()
                .is_ok_and(|l| l.tokens.to_string().contains("test"))
    })
}
