use crate::{
    attr::{ContainerAttrs, FieldAttrs},
    convert::convert_impl,
    desc::struct_desc,
    enum_data::expand_enum,
    field_parts::FieldParts,
    index::indexed_impl,
    named_fields::named_fields,
    refs::{mut_type, ref_type},
    sdata_impl::sdata_impl,
};

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{spanned::Spanned, Data, DeriveInput, Error, Result as SynResult};

pub(super) fn expand_macro(input: DeriveInput) -> SynResult<TokenStream2> {
    if !input.generics.params.is_empty() {
        return Err(Error::new(
            input.generics.span(),
            "#[derive(SData)] does not support generic types yet",
        ));
    }

    let container = ContainerAttrs::parse(&input.attrs)?;

    // `from`/`into`/`try_from` store the type as a target `U`, bypassing shredding.
    if container.delegates() {
        return convert_impl(&input, &container);
    }

    // Enums shred to an externally-tagged object; structs to one node per field.
    if let Data::Enum(data) = &input.data {
        if let Some(index) = container.indexes().first() {
            return Err(Error::new(
                index.name.span(),
                "#[strato(index(...))] is only supported on structs",
            ));
        }

        return expand_enum(&input, data, &container);
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
        .filter(|p| p.attrs().in_shape())
        .map(|p| p.name().to_string())
        .collect();

    let sdata_impl = sdata_impl(name, &ref_name, &mut_name, &parts);
    let ref_type = ref_type(vis, &ref_name, &parts);
    let mut_type = mut_type(vis, &mut_name, &parts);
    let desc = struct_desc(vis, &desc_name, &name.to_string(), &field_names);
    let indexed = indexed_impl(name, &parts, container.indexes())?;

    Ok(quote! {
        #sdata_impl

        #ref_type

        #mut_type

        #desc

        #indexed
    })
}
