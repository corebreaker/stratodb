use super::column_spec::ColumnSpec;
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Error,
    Ident,
    LitStr,
    Token,
    Result as SynResult,
};

/// One comma-separated item inside `index(...)`.
pub(super) enum Item {
    Name(LitStr),
    Columns(Vec<ColumnSpec>),
    Unique,
}

impl Parse for Item {
    fn parse(input: ParseStream) -> SynResult<Self> {
        let key = input.parse::<Ident>()?;
        match key.to_string().as_str() {
            "name" => {
                input.parse::<Token![=]>()?;
                Ok(Item::Name(input.parse()?))
            }
            "columns" => {
                let inner;
                syn::parenthesized!(inner in input);
                let columns = Punctuated::<ColumnSpec, Token![,]>::parse_terminated(&inner)?;

                Ok(Item::Columns(columns.into_iter().collect()))
            }
            "unique" => Ok(Item::Unique),
            _ => Err(Error::new(key.span(), "expected `name`, `columns`, or `unique`")),
        }
    }
}
