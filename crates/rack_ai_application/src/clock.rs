pub trait Clock {
    fn now_text(&self) -> Result<String, String>;
}
