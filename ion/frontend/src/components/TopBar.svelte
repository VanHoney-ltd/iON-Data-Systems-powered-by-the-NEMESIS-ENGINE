<script>
	export let view
	export let viewTitle = ''
	export let selectedCase = ''
	export let cases = []
	export let onCaseChange
	export let canGoBack = false
	export let canGoForward = false
	export let onBack = () => {}
	export let onForward = () => {}
	export let onToggleSidebar = () => {}
	export let onOpenHelp = () => {}

	const viewTitles = {
		dashboard: 'Case Dashboard',
		acquire: 'Acquire Backup',
		decrypt: 'Decrypt Backup',
		agents: 'Run Agents',
		devices: 'Devices',
		apps: 'Apps',
		evidence: 'Evidence',
		reports: 'Reports',
		help: 'Help'
	}

	$: title = viewTitle || viewTitles[view] || view
</script>

<header class="top-bar">
	<div class="left">
		<button class="icon-btn" on:click={onToggleSidebar} title="Toggle sidebar">☰</button>
		<div class="nav-history">
			<button class="icon-btn" on:click={onBack} disabled={!canGoBack} title="Back">←</button>
			<button class="icon-btn" on:click={onForward} disabled={!canGoForward} title="Forward">→</button>
		</div>
		<h1>{title}</h1>
	</div>

	<div class="right">
		{#if cases.length > 0}
			<div class="case-selector">
				<label for="top-case">Case</label>
				<select id="top-case" value={selectedCase} on:change={(e) => onCaseChange(e.target.value)}>
					<option value="">-- Select --</option>
					{#each cases as c}
						<option value={c.id}>{c.name}</option>
					{/each}
				</select>
			</div>
		{/if}
		<button class="icon-btn help" on:click={() => onOpenHelp(view)} title="Help">?</button>
	</div>
</header>

{#if cases.length > 0}
	<div class="mobile-case-bar">
		<select value={selectedCase} on:change={(e) => onCaseChange(e.target.value)}>
			<option value="">-- Select case --</option>
			{#each cases as c}
				<option value={c.id}>{c.name}</option>
			{/each}
		</select>
	</div>
{/if}

<style>
	.top-bar {
		height: 56px;
		background: #0f1419;
		border-bottom: 1px solid #2c3e50;
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0 16px;
		gap: 16px;
		flex-shrink: 0;
	}

	.left, .right {
		display: flex;
		align-items: center;
		gap: 12px;
	}

	.top-bar h1 {
		color: #4cc9f0;
		font-size: 1.1rem;
		margin: 0;
		white-space: nowrap;
	}

	.icon-btn {
		width: 36px;
		height: 36px;
		background: #1a252f;
		border: 1px solid #2c3e50;
		border-radius: 6px;
		color: #e0e0e0;
		cursor: pointer;
		font-size: 1rem;
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.icon-btn:hover:not(:disabled) {
		background: #2c3e50;
	}

	.icon-btn:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}

	.icon-btn.help {
		font-weight: 700;
	}

	.nav-history {
		display: flex;
		gap: 4px;
	}

	.case-selector {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.case-selector label {
		color: #a0a0a0;
		font-size: 0.75rem;
		text-transform: uppercase;
		letter-spacing: 0.5px;
	}

	.case-selector select {
		padding: 8px 12px;
		background: #1a252f;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
		min-width: 160px;
	}

	.mobile-case-bar {
		display: none;
		padding: 8px 12px;
		background: #0f1419;
		border-bottom: 1px solid #2c3e50;
	}

	.mobile-case-bar select {
		width: 100%;
		padding: 8px 12px;
		background: #1a252f;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
	}

	@media (max-width: 768px) {
		.top-bar {
			gap: 8px;
			padding: 0 12px;
		}

		.top-bar h1 {
			font-size: 1rem;
			max-width: 140px;
			overflow: hidden;
			text-overflow: ellipsis;
		}

		.case-selector {
			display: none;
		}

		.mobile-case-bar {
			display: block;
		}

		.icon-btn {
			width: 32px;
			height: 32px;
		}
	}

	@media (max-width: 420px) {
		.nav-history {
			display: none;
		}
	}
</style>
