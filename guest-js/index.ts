import {
	Channel,
	type InvokeArgs,
	Resource,
	invoke,
} from '@tauri-apps/api/core';
import { type ReactivePromise, relay } from 'signalium';

interface TauriInternals {
	unregisterCallback?(id: number): void;
}

declare global {
	interface Window {
		__TAURI_INTERNALS__?: TauriInternals;
	}
}

type SubscriptionArgs = Exclude<
	InvokeArgs,
	number[] | ArrayBuffer | Uint8Array
>;

/** The latest value a `#[subscription]` command streams, subscribed to while
 * something reads it and closed in the backend once nothing does. */
export function subscribe<T>(
	command: string,
	args: SubscriptionArgs = {},
): ReactivePromise<T> {
	return relay<T>(state => {
		const channel = new Channel<T>(value => {
			state.value = value;
		});
		const subscription = invoke<number>(command, { ...args, onEvent: channel });
		return () => {
			subscription
				.then(rid => new Resource(rid).close())
				.catch(e => console.error(`Failed to close ${command}`, e));
			unregisterChannel(channel);
		};
	});
}

// On mobile, Tauri never drops a channel's Rust end, so its callback would
// otherwise stay registered in the webview forever.
function unregisterChannel<T>(channel: Channel<T>): void {
	window.__TAURI_INTERNALS__?.unregisterCallback?.(channel.id);
}
