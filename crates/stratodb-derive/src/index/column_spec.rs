use syn::{Error, Ident};
use syn::parse::{Parse, ParseStream, Result as SynResult};

/// One column of a declared index: a field and its sort direction.
pub(crate) struct ColumnSpec {
    field:      Ident,
    descending: bool,
}

impl ColumnSpec {
    pub(crate) fn field(&self) -> &Ident {
        &self.field
    }

    pub(crate) fn descending(&self) -> bool {
        self.descending
    }
}

impl Parse for ColumnSpec {
    fn parse(input: ParseStream) -> SynResult<Self> {
        let field = input.parse::<Ident>()?;
        let descending = if input.peek(Ident) {
            let direction = input.parse::<Ident>()?;
            match direction.to_string().as_str() {
                "asc" => false,
                "desc" => true,
                _ => return Err(Error::new(direction.span(), "expected `asc` or `desc`")),
            }
        } else {
            false
        };

        Ok(ColumnSpec {
            field,
            descending,
        })
    }
}
