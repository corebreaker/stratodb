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

#[cfg(test)]
mod tests {
    use super::named_fields;
    use syn::{parse_str, DeriveInput};

    fn accepted(src: &str) -> bool {
        named_fields(&parse_str::<DeriveInput>(src).unwrap()).is_ok()
    }

    #[test]
    fn only_named_field_structs_are_accepted() {
        assert!(accepted("struct S { a: u32 }"));

        // Every other shape is rejected (tuple, unit, enum, union).
        assert!(!accepted("struct S(u32);"));
        assert!(!accepted("struct S;"));
        assert!(!accepted("enum E { A }"));
        assert!(!accepted("union U { a: u32 }"));
    }
}
