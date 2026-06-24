use crate::{
    field_parts::FieldParts,
    refs::{mut_type, ref_type},
    named_fields::named_fields,
    sdata_impl::sdata_impl,
};

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{DeriveInput, Error, spanned::Spanned};

pub(super) fn expand_macro(input: DeriveInput) -> syn::Result<TokenStream2> {
    if !input.generics.params.is_empty() {
        return Err(Error::new(
            input.generics.span(),
            "#[derive(SData)] does not support generic types yet",
        ));
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

    let sdata_impl = sdata_impl(name, &ref_name, &mut_name, &parts);
    let ref_type = ref_type(vis, &ref_name, &parts);
    let mut_type = mut_type(vis, &mut_name, &parts);

    Ok(quote! {
        #sdata_impl
        #ref_type
        #mut_type
    })
}
