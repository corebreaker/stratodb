use proc_macro2::Ident;
use syn::Type;

/// A single named field's parts, pre-rendered for the quote templates.
pub(crate) struct FieldParts<'a> {
    /// The field identifier, used as the read getter's name (`x`).
    getter: &'a Ident,
    /// The write getter's name (`x_mut`).
    setter: Ident,
    /// The field's declared type.
    ty:     &'a Type,
    /// The field name as a string literal (`"x"`), used as the path segment.
    name:   String,
}

impl<'a> FieldParts<'a> {
    pub(crate) fn new(getter: &'a Ident, setter: Ident, ty: &'a Type, name: String) -> Self {
        Self {
            getter,
            setter,
            ty,
            name,
        }
    }

    pub(crate) fn getter(&self) -> &'a Ident {
        self.getter
    }

    pub(crate) fn setter(&self) -> &Ident {
        &self.setter
    }

    pub(crate) fn ty(&self) -> &'a Type {
        self.ty
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}
