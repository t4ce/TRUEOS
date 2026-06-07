use alloc::string::String;

#[embassy_executor::task]
pub async fn html_demo_task() {
    let html = super::html_shack::Html::new(
        String::from("html://demo"),
        String::from(trueos_qjs::html_demo::UI_HTML),
    );

    let (ready_len, handed_off) = super::html_shack::enqueue_ready_html_for_browser(html).await;
    crate::log!(
        "html-demo: enqueued ready_queue={} handed_off={}\n",
        ready_len,
        if handed_off { 1 } else { 0 }
    );
}
