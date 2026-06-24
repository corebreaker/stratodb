use syn::{
    punctuated::Punctuated,
    spanned::Spanned,
    token::Comma,
    Data,
    DeriveInput,
    Error,
    Field,
    Fields,
    Result as SynResult,
};

/// Extracts the named fields, rejecting enums, unions and tuple/unit structs.
pub(super) fn named_fields(input: &DeriveInput) -> SynResult<&Punctuated<Field, Comma>> {
    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => Ok(&named.named),
            other => Err(Error::new(
                other.span(),
                "#[derive(SData)] supports only structs with named fields",
            )),
        },
        Data::Enum(_) => Err(Error::new(
            input.ident.span(),
            "#[derive(SData)] does not support enums yet",
        )),
        Data::Union(_) => Err(Error::new(
            input.ident.span(),
            "#[derive(SData)] does not support unions",
        )),
    }
}
