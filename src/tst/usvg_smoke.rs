pub fn run() {
    const INLINE_SVG: &[u8] = br##"
<svg width="64" height="64" viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <!-- Sun gradient -->
    <linearGradient id="retroGrad" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ff2ea6"/>
      <stop offset="100%" stop-color="#ff8c42"/>
    </linearGradient>

    <!-- Mask for sun stripes -->
    <mask id="stripeMask">
      <rect width="64" height="64" fill="white"/>
      <rect x="0" y="26" width="64" height="3" fill="black"/>
      <rect x="0" y="32" width="64" height="3" fill="black"/>
      <rect x="0" y="38" width="64" height="3" fill="black"/>
      <rect x="0" y="44" width="64" height="3" fill="black"/>
    </mask>

    <!-- Background gradient -->
    <linearGradient id="bgGrad" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#12002b"/>
      <stop offset="100%" stop-color="#1a003f"/>
    </linearGradient>
  </defs>

  <!-- Background -->
  <rect width="64" height="64" fill="url(#bgGrad)"/>

  <!-- Sun -->
  <circle cx="32" cy="32" r="20" fill="url(#retroGrad)" mask="url(#stripeMask)"/>
</svg>"##;

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
