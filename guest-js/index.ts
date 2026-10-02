import {
	Channel,
	type InvokeArgs,
	Resource,
	invoke,
} from '@tauri-apps/api/core';
import { type ReactivePromise, reactive, relay } from 'signalium';

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

// Memoized on (command, args), hashed structurally, so every reader of the same
// subscription shares one backend stream.
const subscription = reactive((command: string, args: SubscriptionArgs) =>
	relay<unknown>(state => {
		const channel = new Channel<unknown>(value => {
			state.value = value;
		});
		const subscription = invoke<number>(command, { ...args, onEvent: channel });
		subscription.catch(e => state.setError(e));
		return () => {
			subscription
				.then(
					rid => new Resource(rid).close(),
					() => {},
				)
				.catch(e => console.error(`Failed to close ${command}`, e));
			unregisterChannel(channel);
		};
	}),
);

/** The latest value a `#[subscription]` command streams, subscribed to while
 * something reads it and closed in the backend once nothing does. Rejected
 * with the command's error if the subscription can't be set up. */
export function subscribe<T>(
	command: string,
	args: SubscriptionArgs = {},
): ReactivePromise<T> {
	return subscription(command, args) as ReactivePromise<T>;
}

// On mobile, Tauri never drops a channel's Rust end, so its callback would
// otherwise stay registered in the webview forever.
function unregisterChannel<T>(channel: Channel<T>): void {
	window.__TAURI_INTERNALS__?.unregisterCallback?.(channel.id);
}
