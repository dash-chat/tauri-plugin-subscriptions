use futures::{Stream, StreamExt};
use serde::Serialize;
use tauri::{
    ipc::{Channel, CommandArg, CommandItem, InvokeError},
    plugin::{Builder, TauriPlugin},
    webview::PageLoadEvent,
    Manager, Resource, ResourceId, ResourceTable, Runtime, Webview,
};
pub use tauri_plugin_subscriptions_macros::subscription;
use tokio_util::task::AbortOnDropHandle;

/// Drops every resource a page holds when it reloads, which cancels its
/// subscriptions: Tauri itself only sweeps resource tables at app exit.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("subscriptions")
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Started {
                *webview.resources_table() = ResourceTable::default();
            }
        })
        .build()
}

struct Subscription(#[allow(dead_code)] AbortOnDropHandle<()>);

impl Resource for Subscription {}

/// The frontend end of a subscription: the channel it passed as the command's
/// `onEvent` argument and the webview that invoked it.
pub struct Subscriber<T, R: Runtime> {
    webview: Webview<R>,
    channel: Channel<T>,
}

impl<'de, T, R: Runtime> CommandArg<'de, R> for Subscriber<T, R> {
    fn from_command(command: CommandItem<'de, R>) -> Result<Self, InvokeError> {
        let webview = command.message.webview();
        Ok(Self {
            webview,
            channel: Channel::from_command(command)?,
        })
    }
}

impl<T, R> Subscriber<T, R>
where
    R: Runtime,
    T: Serialize + Clone + Send + Sync + 'static,
{
    /// Forward `stream` to the frontend until it closes the returned resource
    /// (`new Resource(rid).close()`), its page reloads, or the stream ends.
    pub fn subscribe<S>(self, stream: S) -> ResourceId
    where
        S: Stream<Item = T> + Send + 'static,
    {
        let task = tokio::spawn(forward(stream, self.channel));
        self.webview
            .resources_table()
            .add(Subscription(AbortOnDropHandle::new(task)))
    }
}

async fn forward<T, S>(stream: S, channel: Channel<T>)
where
    T: Serialize + Clone,
    S: Stream<Item = T>,
{
    let mut stream = std::pin::pin!(stream);
    while let Some(item) = stream.next().await {
        if let Err(err) = channel.send(item) {
            log::warn!("Failed to send subscription item: {err:?}");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::{App, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
    use tokio::sync::{mpsc, oneshot};

    use super::*;

    fn app_with_webview() -> (App<MockRuntime>, WebviewWindow<MockRuntime>) {
        let app = mock_builder()
            .plugin(init())
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
            .build()
            .unwrap();
        (app, webview)
    }

    /// A stream that yields one item and then stays open, with a receiver that
    /// resolves once the stream is dropped.
    fn open_stream() -> (
        impl Stream<Item = u32> + Send + 'static,
        oneshot::Receiver<()>,
    ) {
        let (dropped_tx, dropped_rx) = oneshot::channel::<()>();
        let stream = futures::stream::once(async { 1 })
            .chain(futures::stream::pending())
            .map(move |n| {
                let _ = &dropped_tx;
                n
            });
        (stream, dropped_rx)
    }

    fn recording_channel() -> (Channel<u32>, mpsc::UnboundedReceiver<()>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let channel = Channel::new(move |_| {
            let _ = tx.send(());
            Ok(())
        });
        (channel, rx)
    }

    fn subscriber(
        webview: &WebviewWindow<MockRuntime>,
        channel: Channel<u32>,
    ) -> Subscriber<u32, MockRuntime> {
        Subscriber {
            webview: webview.as_ref().clone(),
            channel,
        }
    }

    async fn assert_dropped(dropped: oneshot::Receiver<()>) {
        tokio::time::timeout(Duration::from_secs(5), dropped)
            .await
            .expect("stream was not dropped")
            .unwrap_err();
    }

    #[tokio::test]
    async fn closing_the_resource_drops_the_stream() {
        let (_app, webview) = app_with_webview();
        let (stream, dropped) = open_stream();
        let (channel, mut received) = recording_channel();

        let rid = subscriber(&webview, channel).subscribe(stream);
        received.recv().await.unwrap();
        webview.resources_table().close(rid).unwrap();

        assert_dropped(dropped).await;
    }

    #[tokio::test]
    async fn resetting_the_page_resources_drops_every_stream() {
        let (_app, webview) = app_with_webview();
        let (first, first_dropped) = open_stream();
        let (second, second_dropped) = open_stream();
        subscriber(&webview, recording_channel().0).subscribe(first);
        subscriber(&webview, recording_channel().0).subscribe(second);

        *webview.resources_table() = ResourceTable::default();

        assert_dropped(first_dropped).await;
        assert_dropped(second_dropped).await;
    }
}
