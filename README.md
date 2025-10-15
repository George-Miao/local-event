# Local event

A single-threaded (unsync) version of [`event-listener`](https://crates.io/crates/event-listener).

## Get started

Add the crate

```shell
cargo add local-event
```

then

```rust
use local_event::Event;

let event = Event::new();
let listener = event.listen();

// Task 1
event.notify(1);

// Task 2
listener.await;
// Do something after the event is received
```
