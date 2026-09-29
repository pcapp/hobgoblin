# Execute the Read Tool Refactor

Goal: make the current inline `Read` tool execution path more canonical Rust by separating parsing, dispatch, execution, and output handling into small typed pieces.

Note: this document is a planning and learning checklist. Do not edit project code from it unless explicitly asked.

## Checklist

- [x] Run the current program once with a prompt that triggers the `Read` tool and save the observed response shape mentally or in notes.
  - Check: confirm the response contains `choices[0].message.tool_calls`.

- [x] Introduce typed structs for tool-call parsing instead of indexing deeply into `serde_json::Value`.
  - Suggested types: `ChatCompletionResponse`, `Choice`, `Message`, `ToolCall`, `FunctionCall`.
  - Check: `serde_json::from_value::<ChatCompletionResponse>(response)?` compiles.

- [x] Add `serde` as a direct dependency if deriving `Deserialize` or `Serialize`.
  - Suggested dependency:

    ```toml
    serde = { version = "1", features = ["derive"] }
    ```

  - Check: `cargo check` can resolve `serde::Deserialize`.

- [x] Introduce a typed argument struct for the `Read` tool.
  - Suggested type:

    ```rust
    #[derive(serde::Deserialize)]
    struct ReadArgs {
        file_path: String,
    }
    ```

  - Check: `serde_json::from_str::<ReadArgs>(&call.function.arguments)?` compiles.

- [ ] Replace the inline argument-object lookup with typed argument parsing.
  - Current pattern to remove:

    ```rust
    arguments_obj["file_path"].as_str()
    ```

  - Check: there is no direct JSON indexing for `file_path`.

- [ ] Change `read_file_tool` to return a typed `Result<String, std::io::Error>` internally.
  - Suggested implementation:

    ```rust
    fn read_file(file_path: &str) -> Result<String, std::io::Error> {
        std::fs::read_to_string(file_path)
    }
    ```

  - Check: file-reading code does not construct JSON directly.

- [ ] Add a separate adapter that converts the read result into a tool-output JSON value.
  - Suggested shape:

    ```json
    {
      "ok": true,
      "content": "..."
    }
    ```

    or:

    ```json
    {
      "ok": false,
      "error": "..."
    }
    ```

  - Check: JSON construction happens at the API boundary, not inside the file-reading function.

- [ ] Add a small dispatcher function for tool calls.
  - Suggested signature:

    ```rust
    fn execute_tool_call(call: &ToolCall) -> Result<serde_json::Value, Box<dyn std::error::Error>>
    ```

  - Check: `main` no longer knows how to parse `ReadArgs` directly.

- [ ] Match on the tool name inside the dispatcher.
  - Suggested behavior:
    - `"Read"` parses `ReadArgs` and reads the file.
    - Unknown tool names return a structured error JSON.
  - Check: the code handles unknown tools without panicking.

- [ ] Make the missing-file behavior explicit.
  - Recommended behavior: the low-level file reader returns `Err(std::io::Error)`.
  - Recommended JSON adapter output:

    ```json
    {
      "ok": false,
      "error": "No such file or directory (os error 2)"
    }
    ```

  - Check: a missing file never panics and never prints an empty success response.

- [ ] Remove `unwrap()` calls from the tool-call path.
  - Check: `rg "unwrap\\(" src/main.rs` shows no unwraps in the tool execution logic.

- [ ] Keep assistant text output separate from tool-call output.
  - Check: the code path for `message.content` is still simple and readable.

- [ ] Add unit tests for `read_file`.
  - Test: reading an existing temporary file returns its contents.
  - Test: reading a missing file returns an error with kind `std::io::ErrorKind::NotFound`.
  - Suggested test shape:

    ```rust
    #[test]
    fn read_file_returns_contents_for_existing_file() {
        let path = std::env::temp_dir().join("read-file-test.txt");
        std::fs::write(&path, "hello").expect("write test file");

        let contents = read_file(path.to_str().expect("utf-8 path")).expect("read file");

        assert_eq!(contents, "hello");

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn read_file_returns_not_found_for_missing_file() {
        let path = std::env::temp_dir().join("missing-read-file-test.txt");
        std::fs::remove_file(&path).ok();

        let err = read_file(path.to_str().expect("utf-8 path")).expect_err("missing file");

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
    ```

  - Check: both tests pass with `cargo test`.

- [ ] Add unit tests for the JSON adapter.
  - Test: successful read result becomes `{ "ok": true, "content": "..." }`.
  - Test: failed read result becomes `{ "ok": false, "error": "..." }`.
  - Check: tests assert fields using `value["ok"]`, `value["content"]`, and `value["error"]`.

- [ ] Add unit tests for `execute_tool_call`.
  - Test: a `Read` tool call with valid JSON arguments reads the requested file.
  - Test: a `Read` tool call with a missing file returns structured error JSON.
  - Test: an unknown tool name returns structured error JSON.
  - Test: malformed `arguments` JSON returns an error or structured error, whichever policy the refactor chooses.
  - Check: dispatcher behavior is covered without making a real OpenRouter request.

- [x] Run formatting.
  - Command: `cargo fmt`
  - Check: command exits successfully.
  - Verified: `cargo fmt` exited successfully.

- [x] Run the compiler.
  - Command: `cargo check`
  - Check: command exits successfully.
  - Verified: `cargo check` exited successfully, with warnings about unused parsed response structs and `read_file_tool`.

- [ ] Run unit tests.
  - Command: `cargo test`
  - Check: all tests pass.
  - Verified: `cargo test` exited successfully, but it ran `0` tests. Keep this unchecked until real unit tests are added.

- [ ] Manually test a successful read.
  - Example:

    ```sh
    cargo run -- -p "Read src/main.rs"
    ```

  - Check: the program prints the file contents or a clearly structured tool result.

- [ ] Manually test a missing file.
  - Example:

    ```sh
    cargo run -- -p "Read does-not-exist.txt"
    ```

  - Check: the program returns or prints an error instead of panicking.

## Target Shape

After the refactor, `main` should mostly do orchestration:

1. Parse CLI arguments.
2. Load config.
3. Send the chat completion request.
4. Deserialize the response.
5. If the assistant requested tools, pass each tool call to `execute_tool_call`.
6. If the assistant returned content, print the content.

The canonical Rust improvement is to keep fallible operations explicit with `Result`, use `Deserialize` for known JSON shapes, and reserve `serde_json::Value` for flexible API-boundary data.

## Key Rust Concepts

### `Result<T, E>`

Rust uses `Result` for operations that can fail:

```rust
Result<T, E>
```

means either:

```rust
Ok(value)
```

or:

```rust
Err(error)
```

Reading a file can fail, so this is canonical:

```rust
fn read_file(file_path: &str) -> Result<String, std::io::Error> {
    std::fs::read_to_string(file_path)
}
```

The function returns the file contents on success, or an `std::io::Error` on failure.

### The `?` Operator

The `?` operator is shorthand for "return early if this failed":

```rust
let contents = read_file("src/main.rs")?;
```

If `read_file` returns `Ok(contents)`, Rust unwraps the value into `contents`.

If it returns `Err(err)`, the current function returns that error immediately.

This only works inside a function that also returns a compatible `Result`, such as:

```rust
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let contents = read_file("src/main.rs")?;
    println!("{}", contents);
    Ok(())
}
```

### Borrowing with `&str`

This signature:

```rust
fn read_file(file_path: &str) -> Result<String, std::io::Error>
```

means the function borrows a string slice. It does not take ownership of the caller's `String`.

That lets you call it with either:

```rust
read_file("src/main.rs")
```

or:

```rust
let path = String::from("src/main.rs");
read_file(&path)
```

### `serde` vs `serde_json`

`serde` is the general serialization/deserialization framework.

`serde_json` is the JSON-specific implementation.

Use `serde::Deserialize` when you know the expected JSON shape:

```rust
#[derive(serde::Deserialize)]
struct ReadArgs {
    file_path: String,
}
```

Then parse JSON into the Rust type:

```rust
let args: ReadArgs = serde_json::from_str(&call.function.arguments)?;
```

This is usually better than repeatedly indexing into `serde_json::Value`, because the compiler helps you keep the shape correct.

### `serde_json::Value`

`serde_json::Value` is useful when the JSON shape is flexible or when you are building generic JSON output:

```rust
serde_json::json!({
    "ok": true,
    "content": contents
})
```

For this project, a good rule of thumb is:

- Use typed structs for data you expect and control.
- Use `serde_json::Value` at API boundaries or for generic tool output.

### Unit Tests

Rust unit tests usually live in the same file as the code:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_file_returns_not_found_for_missing_file() {
        let err = read_file("definitely-missing.txt").expect_err("missing file");

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
```

`use super::*;` imports the functions from the parent module so the tests can call them.

Run tests with:

```sh
cargo test
```

## References

- Rust Book: Error Handling with `Result`  
  https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html

- Rust Book: The `?` Operator  
  https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html#propagating-errors

- Rust Book: References and Borrowing  
  https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html

- Rust Book: Automated Tests  
  https://doc.rust-lang.org/book/ch11-00-testing.html

- `std::fs::read_to_string` documentation  
  https://doc.rust-lang.org/std/fs/fn.read_to_string.html

- `std::io::ErrorKind` documentation  
  https://doc.rust-lang.org/std/io/enum.ErrorKind.html

- Serde derive guide  
  https://serde.rs/derive.html

- `serde_json::Value` documentation  
  https://docs.rs/serde_json/latest/serde_json/value/enum.Value.html
