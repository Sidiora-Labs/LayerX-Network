pub trait Required<T> {
    fn required(self, context: &str) -> T;
}

impl<T, E> Required<T> for Result<T, E> {
    fn required(self, context: &str) -> T {
        self.unwrap_or_else(|_| panic!("{context}; error contents redacted"))
    }
}

impl<T> Required<T> for Option<T> {
    fn required(self, context: &str) -> T {
        self.unwrap_or_else(|| panic!("{context}"))
    }
}
