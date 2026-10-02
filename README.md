# tauri-plugin-subscriptions

Stream values from Rust to the frontend of a Tauri 2 app, with the backend work
cancelled when the frontend stops listening.

Tauri never tells Rust that a page stopped listening to a `Channel`, and it
leaves a reloaded page's resources in place. This plugin ties each stream to a
webview resource: the frontend closes it when it unsubscribes, and the plugin
drops every resource of a page when it reloads.

## Rust

```rust
tauri::Builder::default().plugin(tauri_plugin_subscriptions::init())
```

A `#[subscription]` function returns a stream and becomes a Tauri command:

```rust
use tauri_plugin_subscriptions::subscription;

#[subscription]
pub fn counter(step: u64) -> impl futures::Stream<Item = u64> {
    futures::stream::iter((0..).step_by(step as usize))
}
```

Register it with `tauri::generate_handler![counter]` like any other command.

## JavaScript

```ts
import { subscribe } from 'tauri-plugin-subscriptions';

const counter = subscribe<number>('counter', { step: 2 }); // signalium ReactivePromise
```

The subscription opens while something reads the reactive value and closes in
the backend once nothing does.
