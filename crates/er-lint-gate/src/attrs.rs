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

/// Whether an item only exists in a test build: `#[cfg(test)]`, or a test
/// harness attribute such as `#[test]` / `#[tokio::test]`.
///
/// A file that is itself a test-only module (`#[cfg(test)] mod tests;`) has no
/// cfg attribute inside it, so its `#[test]` functions are the only signal a
/// per-file scan can see.
pub(crate) fn is_test_item(attrs: &[syn::Attribute]) -> bool {
    is_cfg_test(attrs)
        || attrs.iter().any(|attr| {
            attr.path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "test")
        })
}

/// The attributes an item can carry. `None` for items that cannot be
/// conditionally compiled, which no rule needs to inspect.
pub(crate) fn item_attrs(item: &syn::Item) -> Option<&[syn::Attribute]> {
    match item {
        syn::Item::Const(item) => Some(&item.attrs),
        syn::Item::Enum(item) => Some(&item.attrs),
        syn::Item::ExternCrate(item) => Some(&item.attrs),
        syn::Item::Fn(item) => Some(&item.attrs),
        syn::Item::ForeignMod(item) => Some(&item.attrs),
        syn::Item::Impl(item) => Some(&item.attrs),
        syn::Item::Macro(item) => Some(&item.attrs),
        syn::Item::Mod(item) => Some(&item.attrs),
        syn::Item::Static(item) => Some(&item.attrs),
        syn::Item::Struct(item) => Some(&item.attrs),
        syn::Item::Trait(item) => Some(&item.attrs),
        syn::Item::TraitAlias(item) => Some(&item.attrs),
        syn::Item::Type(item) => Some(&item.attrs),
        syn::Item::Union(item) => Some(&item.attrs),
        syn::Item::Use(item) => Some(&item.attrs),
        _ => None,
    }
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

    #[test]
    fn the_test_harness_attribute_is_test_code() {
        assert!(is_test_item(&fn_attrs("#[test] fn f() {}")));
        assert!(is_test_item(&fn_attrs("#[tokio::test] async fn f() {}")));
        assert!(!is_test_item(&fn_attrs(
            "#[cfg(feature = \"x\")] fn f() {}"
        )));
    }
}
