// generator/context.rs
pub struct GenerationContext {
    pub fields: Vec<TokenStream>,
    pub init_logic: Vec<TokenStream>,
    pub struct_init: Vec<TokenStream>,
}

impl GenerationContext {
    pub fn add_component(&mut self, field: TokenStream, init: TokenStream, struct_assign: TokenStream) {
        self.fields.push(field);
        self.init_logic.push(init);
        self.struct_init.push(struct_assign);
    }
}

