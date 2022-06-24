# mavourings
**m**y f**avour**ite th**ings**. A collection of things I find useful while writing small web applications. Most of them are behind the feature flags. Documentation is mostly incomplete and needs work. This crate was written for myself, but you may find it useful. 

## Features
**`cookie`**  
Pulls in: `time`

Enables the `cookie` module. The cookie parser and header builder. Needs the `time` crate for time formatting.

**`send_file`**  
pulls in: `tokio`, `hyper`, `mime_guess`

Enables the `file_string_reply` function to build a `hyper::Response<Body>` from a file. This function tries to guess the mime type using `mime_guess` and reads the file asynchronously while is why we need `tokio`.

**`template`**  
pulls in: `bempline`  
enables: `send_file`

Enables the `template` module. Read a file in like `send_file`, using the same dependencies, and use it to build a `bempline::Document`. This `Template` can be built into a `hyper::Response`.