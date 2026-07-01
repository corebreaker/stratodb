use crate::{
    attr::{ContainerAttrs, FieldAttrs},
    convert::convert_impl,
    desc::struct_desc,
    enum_data::expand_enum,
    field_parts::FieldParts,
    generics::Generics,
    index::indexed_impl,
    named_fields::named_fields,
    refs::{mut_type, ref_type},
    sdata_impl::sdata_impl,
};

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Error, Result as SynResult};

pub(super) fn expand_macro(input: DeriveInput) -> SynResult<TokenStream2> {
    let container = ContainerAttrs::parse(&input.attrs)?;
    let generics = Generics::analyze(&input.generics, container.bound());

    // `from`/`into`/`try_from` store the type as a target `U`, bypassing shredding.
    if container.delegates() {
        return convert_impl(&input, &container, &generics);
    }

    // Enums shred to an externally-tagged object; structs to one node per field.
    if let Data::Enum(data) = &input.data {
        if let Some(index) = container.indexes().first() {
            return Err(Error::new(
                index.name().span(),
                "#[strato(index(...))] is only supported on structs",
            ));
        }

        return expand_enum(&input, data, &container, &generics);
    }

    // `tag`/`content`/`untagged` describe enum representations.
    if container.tag().is_some() || container.content().is_some() || container.untagged() {
        return Err(Error::new(
            input.ident.span(),
            "`tag`/`content`/`untagged` are only supported on enums",
        ));
    }

    let fields = named_fields(&input)?;
    let mut parts = Vec::with_capacity(fields.len());
    for field in fields {
        let getter = field.ident.as_ref().expect("named field has an identifier");
        let attrs = FieldAttrs::parse(&field.attrs)?;

        // Stored name: an explicit `rename` wins, else `rename_all`, else the field.
        let name = match attrs.rename() {
            Some(rename) => rename.to_string(),
            None => match container.rename_all() {
                Some(rule) => rule.apply_to_field(&getter.to_string()),
                None => getter.to_string(),
            },
        };

        parts.push(FieldParts::new(
            getter,
            format_ident!("{}_mut", getter),
            &field.ty,
            name,
            attrs,
        ));
    }

    let vis = &input.vis;
    let name = &input.ident;
    let ref_name = format_ident!("Strato{}", name);
    let mut_name = format_ident!("Strato{}Mut", name);
    let desc_name = format_ident!("Strato{}Desc", name);

    // The descriptor lists only fields that are part of the stored shape.
    let field_names: Vec<String> = parts
        .iter()
        .filter(|p| p.attrs().in_shape() && !p.attrs().is_flatten())
        .map(|p| p.name().to_string())
        .collect();

    let sdata_impl = sdata_impl(name, &ref_name, &mut_name, &parts, &generics);
    let ref_type = ref_type(vis, &ref_name, &parts, &generics);
    let mut_type = mut_type(vis, &mut_name, &parts, &generics);
    let desc = struct_desc(vis, &desc_name, &name.to_string(), &field_names);
    let indexed = indexed_impl(name, &parts, container.indexes(), &generics)?;

    Ok(quote! {
        #sdata_impl

        #ref_type

        #mut_type

        #desc

        #indexed
    })
}

#[cfg(test)]
mod tests {
    use super::expand_macro;
    use syn::{parse_str, DeriveInput};

    /// Whether the derive rejects `src` (an otherwise well-formed item that misuses
    /// a `#[strato(...)]` attribute). Every rejection is a `return Err(...)` deep in
    /// the pipeline — the same paths that surface as `compile_error!` in real use.
    fn rejected(src: &str) -> bool {
        expand_macro(parse_str::<DeriveInput>(src).unwrap()).is_err()
    }

    #[test]
    fn well_formed_items_expand() {
        assert!(expand_macro(parse_str::<DeriveInput>("struct S { a: u32 }").unwrap()).is_ok());
        assert!(expand_macro(parse_str::<DeriveInput>("enum E { A, B(u32) }").unwrap()).is_ok());
    }

    #[test]
    fn container_attribute_misuse_is_rejected() {
        // `tag`/`content`/`untagged` belong on enums, not structs.
        assert!(rejected(r#"#[strato(tag = "t")] struct S { a: u32 }"#));
        // An index declaration belongs on a struct, not an enum.
        assert!(rejected(r#"#[strato(index(name = "i", columns(a)))] enum E { A }"#));
        // An unknown container attribute, and an unknown `rename_all` rule.
        assert!(rejected(r#"#[strato(bogus)] struct S { a: u32 }"#));
        assert!(rejected(r#"#[strato(rename_all = "bogus")] struct S { a: u32 }"#));
    }

    #[test]
    fn field_attribute_conflicts_are_rejected() {
        assert!(rejected(r#"struct S { #[strato(flatten, rename = "x")] a: u32 }"#));
        assert!(rejected(
            r#"struct S { #[strato(with = "m", store_with = "f")] a: u32 }"#
        ));
        assert!(rejected(r#"struct S { #[strato(skip, with = "m")] a: u32 }"#));
        assert!(rejected(r#"struct S { #[strato(skip_load, load_with = "f")] a: u32 }"#));
        assert!(rejected(r#"struct S { #[strato(bogus)] a: u32 }"#));
    }

    #[test]
    fn index_declaration_errors_are_rejected() {
        assert!(rejected(
            r#"#[strato(index(name = "i", columns(missing)))] struct S { a: u32 }"#
        ));
        assert!(rejected(
            r#"#[strato(index(name = "i", columns()))] struct S { a: u32 }"#
        ));
        assert!(rejected(
            r#"#[strato(index(name = "i", columns(a bogus)))] struct S { a: u32 }"#
        ));
        assert!(rejected(r#"#[strato(index(bogus))] struct S { a: u32 }"#));
    }

    #[test]
    fn convert_declaration_errors_are_rejected() {
        // `from`/`try_from` needs a matching `into`; `into` needs `from`/`try_from`.
        assert!(rejected(r#"#[strato(from = "u32")] struct S(u32);"#));
        assert!(rejected(r#"#[strato(into = "u32")] struct S(u32);"#));
        // `from` and `try_from` are mutually exclusive.
        assert!(rejected(
            r#"#[strato(into = "u32", from = "u32", try_from = "u32")] struct S(u32);"#
        ));
        // A delegated type carries no fields to index.
        assert!(rejected(
            r#"#[strato(into = "u32", from = "u32", index(name = "i", columns(a)))] struct S(u32);"#
        ));
    }

    #[test]
    fn enum_representation_errors_are_rejected() {
        // `content` requires `tag`; `untagged` cannot combine with `tag`.
        assert!(rejected(r#"#[strato(content = "c")] enum E { A }"#));
        assert!(rejected(r#"#[strato(untagged, tag = "t")] enum E { A }"#));
        // The `other` catch-all: at most one, never untagged, must be a unit variant.
        assert!(rejected(
            r#"#[strato(tag = "t")] enum E { #[strato(other)] A, #[strato(other)] B }"#
        ));
        assert!(rejected(r#"#[strato(untagged)] enum E { #[strato(other)] A }"#));
        assert!(rejected(r#"#[strato(tag = "t")] enum E { #[strato(other)] A(u32) }"#));
        // An unknown variant attribute.
        assert!(rejected(r#"enum E { #[strato(bogus)] A }"#));
    }
}
