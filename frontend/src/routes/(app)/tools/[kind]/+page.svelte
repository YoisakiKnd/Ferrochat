<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';

	$: goto(`/tools?tab=${$page.params.kind ?? 'translate'}`, { replaceState: true });
</script>
