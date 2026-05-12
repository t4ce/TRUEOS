use alloc::string::String;

#[embassy_executor::task]
pub async fn html_demo_task() {
    let html = crate::tst_html_shack::Html::new(
        String::from("html://demo"),
        String::from(trueos_qjs::html_demo::UI_HTML),
    );

    let handed_off = crate::tst_html_shack::handoff_html_to_truesurfer(html).await;
    crate::log!("html-demo: handed_off={}\n", if handed_off { 1 } else { 0 });
}
