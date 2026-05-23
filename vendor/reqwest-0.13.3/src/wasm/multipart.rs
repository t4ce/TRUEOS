//! multipart/form-data
use std::borrow::Cow;
use core::fmt;

use http::HeaderMap;
use mime_guess::Mime;
use web_sys::FormData;

use super::Body;

/// An async multipart/form-data request.
pub struct Form {
    inner: FormParts<Part>,
}

impl Form {
    pub(crate) fn is_empty(&self) -> bool {
        self.inner.fields.is_empty()
    }
}

/// A field in a multipart form.
pub struct Part {
    meta: PartMetadata,
    value: Body,
}

pub(crate) struct FormParts<P> {
    pub(crate) fields: Vec<(Cow<'static, str>, P)>,
}

pub(crate) struct PartMetadata {
    mime: Option<Mime>,
    file_name: Option<Cow<'static, str>>,
    pub(crate) headers: HeaderMap,
}

pub(crate) trait PartProps {
    fn metadata(&self) -> &PartMetadata;
}

// ===== impl Form =====

impl Default for Form {
    fn default() -> Self {
        Self::new()
    }
}

impl Form {
    /// Creates a new async Form without any content.
    pub fn new() -> Form {
        Form {
            inner: FormParts::new(),
        }
    }

    /// Add a data field with supplied name and value.
    ///
    /// # Examples
    ///
    /// ```
    /// let form = reqwest::multipart::Form::new()
    ///     .text("username", "seanmonstar")
    ///     .text("password", "secret");
    /// ```
    pub fn text<T, U>(self, name: T, value: U) -> Form
    where
        T: Into<Cow<'static, str>>,
        U: Into<Cow<'static, str>>,
    {
        self.part(name, Part::text(value))
    }

    /// Adds a customized Part.
    pub fn part<T>(self, name: T, part: Part) -> Form
    where
        T: Into<Cow<'static, str>>,
    {
        self.with_inner(move |inner| inner.part(name, part))
    }

    fn with_inner<F>(self, func: F) -> Self
    where
        F: FnOnce(FormParts<Part>) -> FormParts<Part>,
    {
        Form {
            inner: func(self.inner),
        }
    }

    pub(crate) fn to_form_data(&self) -> crate::Result<FormData> {
        let form = FormData::new()
            .map_err(crate::error::wasm)
            .map_err(crate::error::builder)?;

        for (name, part) in self.inner.fields.iter() {
            part.append_to_form(name, &form)
                .map_err(crate::error::wasm)
                .map_err(crate::error::builder)?;
        }
        Ok(form)
    }
}

impl fmt::Debug for Form {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt_fields("Form", f)
    }
}

// ===== impl Part =====

impl Part {
    /// Makes a text parameter.
    pub fn text<T>(value: T) -> Part
    where
        T: Into<Cow<'static, str>>,
    {
        let body = match value.into() {
            Cow::Borrowed(slice) => Body::from(slice),
            Cow::Owned(string) => Body::from(string),
        };
        Part::new(body)
    }

    /// Makes a new parameter from arbitrary bytes.
    pub fn bytes<T>(value: T) -> Part
    where
        T: Into<Cow<'static, [u8]>>,
    {
        let body = match value.into() {
            Cow::Borrowed(slice) => Body::from(slice),
            Cow::Owned(vec) => Body::from(vec),
        };
        Part::new(body)
    }

    /// Makes a new parameter from an arbitrary stream.
    pub fn stream<T: Into<Body>>(value: T) -> Part {
        Part::new(value.into())
    }

    fn new(value: Body) -> Part {
        Part {
            meta: PartMetadata::new(),
            value: value.into_part(),
        }
    }

    /// Tries to set the mime of this part.
    pub fn mime_str(self, mime: &str) -> crate::Result<Part> {
        Ok(self.mime(mime.parse().map_err(crate::error::builder)?))
    }

    // Re-export when mime 0.4 is available, with split MediaType/MediaRange.
    fn mime(self, mime: Mime) -> Part {
        self.with_inner(move |inner| inner.mime(mime))
    }

    /// Sets the filename, builder style.
    pub fn file_name<T>(self, filename: T) -> Part
    where
        T: Into<Cow<'static, str>>,
    {
        self.with_inner(move |inner| inner.file_name(filename))
    }

    /// Sets custom headers for the part.
    pub fn headers(self, headers: HeaderMap) -> Part {
        self.with_inner(move |inner| inner.headers(headers))
    }

    fn with_inner<F>(self, func: F) -> Self
    where
        F: FnOnce(PartMetadata) -> PartMetadata,
    {
        Part {
            meta: func(self.meta),
            value: self.value,
        }
    }

    fn append_to_form(
        &self,
        name: &str,
        form: &web_sys::FormData,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let single = self
            .value
            .as_single()
            .expect("A part's body can't be multipart itself");

        let mut mime_type = self.metadata().mime.as_ref();

        // The JS fetch API doesn't support file names and mime types for strings. So we do our best
        // effort to use `append_with_str` and fallback to `append_with_blob_*` if that's not
        // possible.
        if let super::body::Single::Text(text) = single {
            if mime_type.is_none() || mime_type == Some(&mime_guess::mime::TEXT_PLAIN) {
                if self.metadata().file_name.is_none() {
                    return form.append_with_str(name, text);
                }
            } else {
                mime_type = Some(&mime_guess::mime::TEXT_PLAIN);
            }
        }

        let blob = self.blob(mime_type)?;

        if let Some(file_name) = &self.metadata().file_name {
            form.append_with_blob_and_filename(name, &blob, file_name)
        } else {
            form.append_with_blob(name, &blob)
        }
    }

    fn blob(&self, mime_type: Option<&Mime>) -> crate::Result<web_sys::Blob> {
        use web_sys::Blob;
        use web_sys::BlobPropertyBag;
        let mut properties = BlobPropertyBag::new();
        if let Some(mime) = mime_type {
            properties.type_(mime.as_ref());
        }

        let js_value = self
            .value
            .as_single()
            .expect("A part's body can't be set to a multipart body")
            .to_js_value();

        let body_array = js_sys::Array::new();
        body_array.push(&js_value);

        Blob::new_with_u8_array_sequence_and_options(body_array.as_ref(), &properties)
            .map_err(crate::error::wasm)
            .map_err(crate::error::builder)
    }
}

impl fmt::Debug for Part {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut dbg = f.debug_struct("Part");
        dbg.field("value", &self.value);
        self.meta.fmt_fields(&mut dbg);
        dbg.finish()
    }
}

impl PartProps for Part {
    fn metadata(&self) -> &PartMetadata {
        &self.meta
    }
}

// ===== impl FormParts =====

impl<P: PartProps> FormParts<P> {
    pub(crate) fn new() -> Self {
        FormParts { fields: Vec::new() }
    }

    /// Adds a customized Part.
    pub(crate) fn part<T>(mut self, name: T, part: P) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        self.fields.push((name.into(), part));
        self
    }
}

impl<P: fmt::Debug> FormParts<P> {
    pub(crate) fn fmt_fields(&self, ty_name: &'static str, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct(ty_name)
            .field("parts", &self.fields)
            .finish()
    }
}

// ===== impl PartMetadata =====

impl PartMetadata {
    pub(crate) fn new() -> Self {
        PartMetadata {
            mime: None,
            file_name: None,
            headers: HeaderMap::default(),
        }
    }

    pub(crate) fn mime(mut self, mime: Mime) -> Self {
        self.mime = Some(mime);
        self
    }

    pub(crate) fn file_name<T>(mut self, filename: T) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        self.file_name = Some(filename.into());
        self
    }

    pub(crate) fn headers<T>(mut self, headers: T) -> Self
    where
        T: Into<HeaderMap>,
    {
        self.headers = headers.into();
        self
    }
}

impl PartMetadata {
    pub(crate) fn fmt_fields<'f, 'fa, 'fb>(
        &self,
        debug_struct: &'f mut fmt::DebugStruct<'fa, 'fb>,
    ) -> &'f mut fmt::DebugStruct<'fa, 'fb> {
        debug_struct
            .field("mime", &self.mime)
            .field("file_name", &self.file_name)
            .field("headers", &self.headers)
    }
}
