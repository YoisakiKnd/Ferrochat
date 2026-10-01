<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { createEventDispatcher, getContext, onMount, onDestroy } from 'svelte';
	import { config, settings } from '$lib/stores';
	import { blobToFile } from '$lib/utils';
	import { transcribeAudio } from '$lib/apis/audio';

	const i18n = getContext<import('svelte/store').Writable<import('i18next').i18n>>('i18n');
	const dispatch = createEventDispatcher();
	export let recording = false;
	export let className = ' p-2.5 w-full max-w-full';

	type Recognition = {
		continuous: boolean;
		onresult:
			| ((event: {
					results: ArrayLike<ArrayLike<{ transcript: string }>>;
					resultIndex: number;
			  }) => void)
			| null;
		onend: (() => void) | null;
		onerror: ((event: { error: string }) => void) | null;
		start: () => void;
		stop: () => void;
	};
	let loading = true;
	let confirmed = false;
	let cancelled = false;
	let durationSeconds = 0;
	let durationCounter: ReturnType<typeof setInterval>;
	let transcription = '';
	let stream: MediaStream | null = null;
	let mediaRecorder: MediaRecorder | null = null;
	let speechRecognition: Recognition | null = null;
	let audioContext: AudioContext | null = null;
	let frameId = 0;
	let inactivityTimer: ReturnType<typeof setTimeout>;
	const transcriptionAbort = new AbortController();
	let VISUALIZER_BUFFER_LENGTH = 300;
	let visualizerData = Array(VISUALIZER_BUFFER_LENGTH).fill(0);
	let resizeObserver: ResizeObserver;
	let containerWidth = 0;

	const formatSeconds = (seconds: number) =>
		`${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;

	const releaseResources = () => {
		clearInterval(durationCounter);
		clearTimeout(inactivityTimer);
		cancelAnimationFrame(frameId);
		stream?.getTracks().forEach((track) => track.stop());
		stream = null;
		audioContext?.close().catch(() => null);
		audioContext = null;
		if (speechRecognition) {
			speechRecognition.onend = null;
			speechRecognition.onerror = null;
			speechRecognition.onresult = null;
			try {
				speechRecognition.stop();
			} catch {
				// Recognition may not have started when initialization failed.
			}
			speechRecognition = null;
		}
	};

	const stopRecording = () => {
		cancelled = true;
		transcriptionAbort.abort();
		if (mediaRecorder && mediaRecorder.state !== 'inactive') {
			mediaRecorder.onstop = null;
			mediaRecorder.ondataavailable = null;
			mediaRecorder.stop();
		}
		releaseResources();
		recording = false;
	};

	const failRecording = (error: unknown) => {
		if (cancelled) return;
		toast.error(String(error));
		stopRecording();
		dispatch('cancel');
	};

	const analyseAudio = (sourceStream: MediaStream) => {
		audioContext = new AudioContext();
		const source = audioContext.createMediaStreamSource(sourceStream);
		const analyser = audioContext.createAnalyser();
		source.connect(analyser);
		const samples = new Uint8Array(analyser.fftSize);
		const processFrame = () => {
			if (cancelled || loading) return;
			analyser.getByteTimeDomainData(samples);
			const rms = Math.sqrt(
				samples.reduce((sum, value) => sum + ((value - 128) / 128) ** 2, 0) / samples.length
			);
			visualizerData = [
				...visualizerData.slice(-(VISUALIZER_BUFFER_LENGTH - 1)),
				Math.min(1, (rms * 10) ** 1.5)
			];
			frameId = requestAnimationFrame(processFrame);
		};
		frameId = requestAnimationFrame(processFrame);
	};

	const finishBrowserRecording = () => {
		if (cancelled) return;
		releaseResources();
		recording = false;
		if (transcription.trim()) dispatch('confirm', { text: transcription.trim() });
		else dispatch('cancel');
	};

	const confirmRecording = () => {
		if (loading || confirmed || cancelled) return;
		confirmed = true;
		loading = true;
		clearInterval(durationCounter);
		clearTimeout(inactivityTimer);
		if (speechRecognition) speechRecognition.stop();
		else if (mediaRecorder?.state === 'recording') mediaRecorder.stop();
	};

	const startRecording = async () => {
		const web = ($settings?.audio?.stt?.engine || $config?.audio?.stt?.engine || 'web') === 'web';
		const speechWindow = window as typeof window & {
			SpeechRecognition?: new () => Recognition;
			webkitSpeechRecognition?: new () => Recognition;
		};
		const Recognition = speechWindow.SpeechRecognition ?? speechWindow.webkitSpeechRecognition;
		try {
			if (web && !Recognition)
				throw $i18n.t('Speech recognition is not supported in this browser.');
			const acquired = await navigator.mediaDevices.getUserMedia({
				audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true }
			});
			if (cancelled) {
				acquired.getTracks().forEach((track) => track.stop());
				return;
			}
			stream = acquired;
			if (web && Recognition) {
				speechRecognition = new Recognition();
				speechRecognition.continuous = true;
				speechRecognition.onresult = (event) => {
					clearTimeout(inactivityTimer);
					for (let i = event.resultIndex; i < event.results.length; i++) {
						transcription += `${event.results[i][0].transcript} `;
					}
					inactivityTimer = setTimeout(() => speechRecognition?.stop(), 2000);
				};
				speechRecognition.onend = finishBrowserRecording;
				speechRecognition.onerror = (event) =>
					failRecording($i18n.t('Speech recognition error: {{error}}', { error: event.error }));
				speechRecognition.start();
			} else {
				const recorder = new MediaRecorder(acquired);
				mediaRecorder = recorder;
				const chunks: BlobPart[] = [];
				recorder.ondataavailable = (event) => chunks.push(event.data);
				recorder.onstop = async () => {
					releaseResources();
					if (!confirmed || cancelled) return;
					try {
						const mime = recorder.mimeType || 'audio/webm';
						const extension = mime.includes('mp4') ? 'mp4' : mime.includes('ogg') ? 'ogg' : 'webm';
						const file = blobToFile(new Blob(chunks, { type: mime }), `recording.${extension}`);
						const result = await transcribeAudio(
							localStorage.token,
							file,
							transcriptionAbort.signal
						);
						if (cancelled) return;
						if (!result || typeof result.text !== 'string')
							throw $i18n.t('Unable to transcribe this recording.');
						recording = false;
						dispatch('confirm', result);
					} catch (error) {
						failRecording(error);
					}
				};
				recorder.start();
			}
			loading = false;
			durationCounter = setInterval(() => durationSeconds++, 1000);
			analyseAudio(acquired);
		} catch (error) {
			failRecording(error);
		}
	};

	onMount(() => {
		resizeObserver = new ResizeObserver(() => {
			VISUALIZER_BUFFER_LENGTH = Math.max(2, Math.floor(window.innerWidth / 4));
			visualizerData = visualizerData.slice(-VISUALIZER_BUFFER_LENGTH);
		});
		resizeObserver.observe(document.body);
		if (recording) startRecording();
	});

	onDestroy(() => {
		resizeObserver?.disconnect();
		stopRecording();
	});
</script>

<div
	bind:clientWidth={containerWidth}
	class="{loading
		? ' bg-gray-100/50 dark:bg-gray-850/50'
		: 'bg-indigo-300/10 dark:bg-indigo-500/10 '} rounded-full flex justify-between {className}"
>
	<div class="flex items-center mr-1">
		<button
			type="button"
			aria-label={$i18n.t('Cancel recording')}
			class="p-1.5

            {loading
				? ' bg-gray-200 dark:bg-gray-700/50'
				: 'bg-indigo-400/20 text-indigo-600 dark:text-indigo-300 '} 


             rounded-full"
			on:click={async () => {
				stopRecording();
				dispatch('cancel');
			}}
		>
			<svg
				xmlns="http://www.w3.org/2000/svg"
				fill="none"
				viewBox="0 0 24 24"
				stroke-width="3"
				stroke="currentColor"
				class="size-4"
			>
				<path stroke-linecap="round" stroke-linejoin="round" d="M6 18 18 6M6 6l12 12" />
			</svg>
		</button>
	</div>

	<div
		class="flex flex-1 self-center items-center justify-between ml-2 mx-1 overflow-hidden h-6"
		dir="rtl"
	>
		<div
			class="flex items-center gap-0.5 h-6 w-full max-w-full overflow-hidden overflow-x-hidden flex-wrap"
		>
			{#each visualizerData.slice().reverse() as rms}
				<div class="flex items-center h-full">
					<div
						class="w-[2px] shrink-0
                    
                    {loading
							? ' bg-gray-500 dark:bg-gray-400   '
							: 'bg-indigo-500 dark:bg-indigo-400  '} 
                    
                    inline-block h-full"
						style="height: {Math.min(100, Math.max(14, rms * 100))}%;"
					/>
				</div>
			{/each}
		</div>
	</div>

	<div class="flex">
		<div class="  mx-1.5 pr-1 flex justify-center items-center">
			<div
				class="text-sm
        
        
        {loading ? ' text-gray-500  dark:text-gray-400  ' : ' text-indigo-400 '} 
       font-medium flex-1 mx-auto text-center"
			>
				{formatSeconds(durationSeconds)}
			</div>
		</div>

		<div class="flex items-center">
			{#if loading}
				<div class=" text-gray-500 rounded-full cursor-not-allowed">
					<svg
						width="24"
						height="24"
						viewBox="0 0 24 24"
						xmlns="http://www.w3.org/2000/svg"
						fill="currentColor"
						><style>
							.spinner_OSmW {
								transform-origin: center;
								animation: spinner_T6mA 0.75s step-end infinite;
							}
							@keyframes spinner_T6mA {
								8.3% {
									transform: rotate(30deg);
								}
								16.6% {
									transform: rotate(60deg);
								}
								25% {
									transform: rotate(90deg);
								}
								33.3% {
									transform: rotate(120deg);
								}
								41.6% {
									transform: rotate(150deg);
								}
								50% {
									transform: rotate(180deg);
								}
								58.3% {
									transform: rotate(210deg);
								}
								66.6% {
									transform: rotate(240deg);
								}
								75% {
									transform: rotate(270deg);
								}
								83.3% {
									transform: rotate(300deg);
								}
								91.6% {
									transform: rotate(330deg);
								}
								100% {
									transform: rotate(360deg);
								}
							}
						</style><g class="spinner_OSmW"
							><rect x="11" y="1" width="2" height="5" opacity=".14" /><rect
								x="11"
								y="1"
								width="2"
								height="5"
								transform="rotate(30 12 12)"
								opacity=".29"
							/><rect
								x="11"
								y="1"
								width="2"
								height="5"
								transform="rotate(60 12 12)"
								opacity=".43"
							/><rect
								x="11"
								y="1"
								width="2"
								height="5"
								transform="rotate(90 12 12)"
								opacity=".57"
							/><rect
								x="11"
								y="1"
								width="2"
								height="5"
								transform="rotate(120 12 12)"
								opacity=".71"
							/><rect
								x="11"
								y="1"
								width="2"
								height="5"
								transform="rotate(150 12 12)"
								opacity=".86"
							/><rect x="11" y="1" width="2" height="5" transform="rotate(180 12 12)" /></g
						></svg
					>
				</div>
			{:else}
				<button
					type="button"
					aria-label={$i18n.t('Confirm recording')}
					class="p-1.5 bg-indigo-500 text-white dark:bg-indigo-500 dark:text-blue-950 rounded-full"
					on:click={async () => {
						await confirmRecording();
					}}
				>
					<svg
						xmlns="http://www.w3.org/2000/svg"
						fill="none"
						viewBox="0 0 24 24"
						stroke-width="2.5"
						stroke="currentColor"
						class="size-4"
					>
						<path stroke-linecap="round" stroke-linejoin="round" d="m4.5 12.75 6 6 9-13.5" />
					</svg>
				</button>
			{/if}
		</div>
	</div>
</div>

<style>
	.visualizer {
		display: flex;
		height: 100%;
	}

	.visualizer-bar {
		width: 2px;
		background-color: #4a5aba; /* or whatever color you need */
	}
</style>
