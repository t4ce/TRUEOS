#![cfg(feature = "trueos")]

pub const UI_HELLOWORLD_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Hello World</title>
</head>
<body>

<p><a href="/html">Back to HTML demo</a></p>

<h1>Hello World</h1>
<p>This page is served from embedded HTML inside TRUEOS.</p>

</body>
</html>
"##;
