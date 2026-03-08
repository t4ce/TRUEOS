pub fn run() {
    const INLINE_SVG: &[u8] = br#"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'><rect x='4' y='4' width='24' height='24' fill='#3a7'/></svg>"#;

    let options = usvg::Options::default();
    match usvg::Tree::from_data(INLINE_SVG, &options) {
        Ok(tree) => {
            let size = tree.size();
            crate::log!(
                "usvg-smoke: ok width={} height={}\n",
                size.width(),
                size.height()
            );
        }
        Err(err) => {
            crate::log!("usvg-smoke: err {}\n", err);
        }
    }
}
