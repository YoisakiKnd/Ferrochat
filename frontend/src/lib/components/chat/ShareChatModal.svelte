<script lang="ts">
	import { getContext } from 'svelte';
	import { models, config } from '$lib/stores';

	import { toast } from 'svelte-sonner';
	import { deleteSharedChatById, getChatById, shareChatById } from '$lib/apis/chats';
	import { copyToClipboard } from '$lib/utils';

	import Modal from '../common/Modal.svelte';
	import Link from '../icons/Link.svelte';

	export let chatId;

	let chat = null;
	let shareUrl = null;
	let pending = false;
	let loadError = '';
	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');

	const shareLocalChat = async () => {
		const sharedChat = await shareChatById(localStorage.token, chatId);
		shareUrl = `${window.location.origin}/s/${sharedChat.share_id}`;
		chat = sharedChat;

		return shareUrl;
	};

	const shareChat = async () => {
		const _chat = chat.chat;
		console.log('share', _chat);

		toast.success($i18n.t('Redirecting you to Ferrochat Community'));
		const url = 'https://github.com';
		// const url = 'http://localhost:5173';

		const tab = await window.open(`${url}/chats/upload`, '_blank');
		window.addEventListener(
			'message',
			(event) => {
				if (event.origin !== url) return;
				if (event.data === 'loaded') {
					tab.postMessage(
						JSON.stringify({
							chat: _chat,
							models: $models.filter((m) => _chat.models.includes(m.id))
						}),
						'*'
					);
				}
			},
			false
		);
	};

	export let show = false;

	const loadChat = async (id) => {
		chat = null;
		shareUrl = null;
		loadError = '';
		if (!id) return;
		try {
			chat = await getChatById(localStorage.token, id);
		} catch (error) {
			loadError = String(error);
		}
	};

	$: if (show) loadChat(chatId);

	const copyShareLink = async () => {
		if (pending) return;
		pending = true;
		try {
			const isSafari = /^((?!chrome|android).)*safari/i.test(navigator.userAgent);
			let copied;
			if (isSafari && navigator.clipboard?.write && typeof ClipboardItem !== 'undefined') {
				const url = shareLocalChat().then((value) => new Blob([value], { type: 'text/plain' }));
				await navigator.clipboard.write([new ClipboardItem({ 'text/plain': url })]);
				copied = true;
			} else {
				copied = await copyToClipboard(await shareLocalChat());
			}
			if (copied) {
				toast.success($i18n.t('Copied shared chat URL to clipboard!'));
				show = false;
			} else {
				toast.error(
					$i18n.t(
						'Clipboard write permission denied. Please check your browser settings to grant the necessary access.'
					)
				);
			}
		} catch (error) {
			toast.error(String(error));
		} finally {
			pending = false;
		}
	};

	const deleteShareLink = async () => {
		if (pending) return;
		pending = true;
		try {
			chat = await deleteSharedChatById(localStorage.token, chatId);
			shareUrl = null;
		} catch (error) {
			toast.error(String(error));
		} finally {
			pending = false;
		}
	};
</script>

<Modal bind:show size="md">
	<div>
		<div class=" flex justify-between dark:text-gray-300 px-5 pt-4 pb-0.5">
			<div class=" text-lg font-medium self-center">{$i18n.t('Share Chat')}</div>
			<button
				class="self-center"
				on:click={() => {
					show = false;
				}}
			>
				<svg
					xmlns="http://www.w3.org/2000/svg"
					viewBox="0 0 20 20"
					fill="currentColor"
					class="w-5 h-5"
				>
					<path
						d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"
					/>
				</svg>
			</button>
		</div>

		{#if chat}
			<div class="px-5 pt-4 pb-5 w-full flex flex-col justify-center">
				<div class=" text-sm dark:text-gray-300 mb-1">
					{#if chat.share_id}
						<a href="/s/{chat.share_id}" target="_blank"
							>{$i18n.t('You have shared this chat')}
							<span class=" underline">{$i18n.t('before')}</span>.</a
						>
						{$i18n.t('Click here to')}
						<button disabled={pending} class="underline" on:click={deleteShareLink}
							>{$i18n.t('delete this link')}
						</button>
						{$i18n.t('and create a new shared link.')}
					{:else}
						{$i18n.t(
							"Messages you send after creating your link won't be shared. Users with the URL will be able to view the shared chat."
						)}
					{/if}
				</div>

				{#if shareUrl}
					<input
						aria-label={$i18n.t('Shared chat URL')}
						class="w-full rounded-lg p-2 text-sm dark:bg-gray-850"
						readonly
						value={shareUrl}
						on:focus={(event) => event.currentTarget.select()}
					/>
				{/if}

				<div class="flex justify-end">
					<div class="flex flex-col items-end space-x-1 mt-3">
						<div class="flex gap-1">
							{#if $config?.features.enable_community_sharing}
								<button
									class="self-center flex items-center gap-1 px-3.5 py-2 text-sm font-medium bg-gray-100 hover:bg-gray-200 text-gray-800 dark:bg-gray-850 dark:text-white dark:hover:bg-gray-800 transition rounded-full"
									type="button"
									on:click={() => {
										shareChat();
										show = false;
									}}
								>
									{$i18n.t('Share to Ferrochat Community')}
								</button>
							{/if}

							<button
								class="self-center flex items-center gap-1 px-3.5 py-2 text-sm font-medium bg-black hover:bg-gray-900 text-white dark:bg-white dark:text-black dark:hover:bg-gray-100 transition rounded-full"
								type="button"
								id="copy-and-share-chat-button"
								disabled={pending}
								on:click={copyShareLink}
							>
								<Link />

								{#if chat.share_id}
									{$i18n.t('Update and Copy Link')}
								{:else}
									{$i18n.t('Copy Link')}
								{/if}
							</button>
						</div>
					</div>
				</div>
			</div>
		{:else if loadError}
			<div class="px-5 py-4">
				<p role="alert">{loadError}</p>
				<button on:click={() => loadChat(chatId)}>{$i18n.t('Retry')}</button>
			</div>
		{:else}
			<p class="px-5 py-4">{$i18n.t('Loading...')}</p>
		{/if}
	</div>
</Modal>
