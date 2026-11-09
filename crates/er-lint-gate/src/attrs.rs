//! Attribute predicates shared by the gate's source checks.

use syn::punctuated::Punctuated;

/// Whether an item carries `#[cfg(...)]` that can only hold in a test build.
///
/// Only a predicate satisfied solely under tests keeps the item out of every
/// rule: `test`, `all(test, ..)`, or an `any` whose every arm is test-only.
/// `not(test)` and an `any` with a non-test arm are production code.
pub(crate) fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .meta
                .require_list()
                .ok()
                .and_then(|list| syn::parse2::<syn::Meta>(list.tokens.clone()).ok())
                .is_some_and(|meta| predicate_is_test_only(&meta))
    })
}

/// True when the predicate cannot hold outside a test build.
///
/// `all` holds only when every arm holds, so one test-only arm keeps the whole
/// predicate in test builds. `any` can hold outside tests unless every arm is
/// test-only. `not(..)` and value predicates can hold in production.
fn predicate_is_test_only(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) => {
            let Some(name) = list.path.get_ident() else {
                return false;
            };
            let Ok(nested) =
                list.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
            else {
                return false;
            };
            if name == "all" {
                nested.iter().any(predicate_is_test_only)
            } else if name == "any" {
                !nested.is_empty() && nested.iter().all(predicate_is_test_only)
            } else {
                false
            }
        }
        syn::Meta::NameValue(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fn_attrs(source: &str) -> Vec<syn::Attribute> {
        syn::parse_str::<syn::ItemFn>(source).unwrap().attrs
    }

    #[test]
    fn cfg_not_test_is_production_code() {
        assert!(!is_cfg_test(&fn_attrs("#[cfg(not(test))] fn f() {}")));
    }

    #[test]
    fn cfg_any_test_and_feature_is_production_code() {
        assert!(!is_cfg_test(&fn_attrs(
            "#[cfg(any(test, feature = \"x\"))] fn f() {}"
        )));
    }

    #[test]
    fn cfg_all_test_and_unix_is_test_code() {
        assert!(is_cfg_test(&fn_attrs("#[cfg(all(test, unix))] fn f() {}")));
    }
}
