use crate::{
    desc::struct_desc,
    enum_data::expand_enum,
    field_parts::FieldParts,
    index::{index_attrs, indexed_impl},
    named_fields::named_fields,
    refs::{mut_type, ref_type},
    sdata_impl::sdata_impl,
};

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{spanned::Spanned, Data, DeriveInput, Error, Ident, Result as SynResult};

pub(super) fn expand_macro(input: DeriveInput) -> SynResult<TokenStream2> {
    if !input.generics.params.is_empty() {
        return Err(Error::new(
            input.generics.span(),
            "#[derive(SData)] does not support generic types yet",
        ));
    }

    let indexes = index_attrs(&input.attrs)?;

    // Enums shred to an externally-tagged object; structs to one node per field.
    if let Data::Enum(data) = &input.data {
        if let Some(index) = indexes.first() {
            return Err(Error::new(
                index.name.span(),
                "#[strato(index(...))] is only supported on structs",
            ));
        }

        return expand_enum(&input, data);
    }

    let fields = named_fields(&input)?;
    let parts: Vec<FieldParts> = fields
        .iter()
        .map(|field| {
            let getter = field.ident.as_ref().expect("named field has an identifier");

            FieldParts::new(getter, format_ident!("{}_mut", getter), &field.ty, getter.to_string())
        })
        .collect();

    let vis = &input.vis;
    let name = &input.ident;
    let ref_name = format_ident!("Strato{}", name);
    let mut_name = format_ident!("Strato{}Mut", name);
    let desc_name = format_ident!("Strato{}Desc", name);

    let field_names: Vec<String> = parts.iter().map(|p| p.name().to_string()).collect();
    let field_idents: Vec<Ident> = fields
        .iter()
        .map(|field| field.ident.clone().expect("named field has an identifier"))
        .collect();

    let sdata_impl = sdata_impl(name, &ref_name, &mut_name, &parts);
    let ref_type = ref_type(vis, &ref_name, &parts);
    let mut_type = mut_type(vis, &mut_name, &parts);
    let desc = struct_desc(vis, &desc_name, &name.to_string(), &field_names);
    let indexed = indexed_impl(name, &field_idents, &indexes)?;

    Ok(quote! {
        #sdata_impl

        #ref_type

        #mut_type

        #desc

        #indexed
    })
}
