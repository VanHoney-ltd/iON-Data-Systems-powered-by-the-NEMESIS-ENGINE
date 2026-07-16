<script>
	export let agent
	export let caseName
	export let running
	export let status
	export let progress
	export let logs
	export let onCancel
	export let onViewEvidence

	$: completed = status === 'Extraction Complete!'
	$: errored = typeof status === 'string' && status.startsWith('Error:')
	$: percent = progress.total > 0 ? Math.round((progress.step / progress.total) * 100) : 0

	let showLogs = true
</script>

<div class="monitor">
	<h2>Run Monitor</h2>

	{#if !agent}
		<div class="idle">
			<p>No agent is running. Go to <strong>Run Agents</strong> and select one.</p>
		</div>
	{:else}
		<div class="run-info">
			<div class="row">
				<span class="label">Agent</span>
				<span class="value">{agent}</span>
			</div>
			<div class="row">
				<span class="label">Case</span>
				<span class="value">{caseName}</span>
			</div>
			<div class="row">
				<span class="label">Status</span>
				<span class="value status" class:running class:completed class:errored>{status}</span>
			</div>
		</div>

		{#if running || completed || errored}
			<div class="progress-section">
				<div class="progress-bar">
					<div class="progress-fill" style="width: {percent}%"></div>
				</div>
				<div class="progress-meta">
					<span>{progress.label || 'Working...'}</span>
					<span>{progress.step}/{progress.total || '?'}</span>
				</div>
			</div>
		{/if}

		<div class="actions">
			{#if running}
				<button class="btn-danger" on:click={onCancel}>Cancel Run</button>
			{:else if completed}
				<button class="btn-primary" on:click={onViewEvidence}>View Evidence</button>
			{:else if errored}
				<button class="btn-secondary" on:click={() => showLogs = true}>Show Logs</button>
			{/if}
		</div>

		<div class="logs-panel">
			<div class="logs-header">
				<h3>Live Logs</h3>
				<button class="btn-text" on:click={() => showLogs = !showLogs}>
					{showLogs ? 'Hide' : 'Show'}
				</button>
			</div>
			{#if showLogs}
				<div class="logs">
					{#each logs as log}
						<div class="log-entry">{log}</div>
					{:else}
						<div class="log-entry empty">No logs yet.</div>
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</div>

<style>
	.monitor {
		padding: 24px;
	}

	.monitor h2 {
		color: #4cc9f0;
		margin: 0 0 20px;
	}

	.idle {
		background: #1a252f;
		border-radius: 8px;
		padding: 40px;
		text-align: center;
		color: #a0a0a0;
	}

	.run-info {
		background: #1a252f;
		border-radius: 8px;
		padding: 20px;
		margin-bottom: 20px;
	}

	.row {
		display: flex;
		justify-content: space-between;
		padding: 8px 0;
		border-bottom: 1px solid #2c3e50;
	}

	.row:last-child {
		border-bottom: none;
	}

	.label {
		color: #777;
	}

	.value {
		color: #e0e0e0;
		font-weight: 600;
	}

	.status.running {
		color: #ff9f1c;
	}

	.status.completed {
		color: #2ec4b6;
	}

	.status.errored {
		color: #ff6b6b;
	}

	.progress-section {
		margin-bottom: 20px;
	}

	.progress-bar {
		width: 100%;
		height: 12px;
		background: #2c3e50;
		border-radius: 6px;
		overflow: hidden;
	}

	.progress-fill {
		height: 100%;
		background: linear-gradient(90deg, #4cc9f0, #2ec4b6);
		transition: width 0.3s ease;
	}

	.progress-meta {
		display: flex;
		justify-content: space-between;
		margin-top: 8px;
		color: #a0a0a0;
		font-size: 0.85rem;
	}

	.actions {
		margin-bottom: 20px;
	}

	.logs-panel {
		background: #1a252f;
		border-radius: 8px;
		padding: 16px;
	}

	.logs-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 12px;
	}

	.logs-header h3 {
		color: #4cc9f0;
		margin: 0;
	}

	.logs {
		height: 320px;
		overflow-y: auto;
		background: #0f1419;
		border: 1px solid #2c3e50;
		border-radius: 4px;
		padding: 12px;
		font-family: 'Courier New', Courier, monospace;
		font-size: 0.85rem;
	}

	.log-entry {
		padding: 4px 0;
		color: #a0a0a0;
		border-bottom: 1px solid rgba(44, 62, 80, 0.3);
	}

	.log-entry.empty {
		color: #555;
		font-style: italic;
		border: none;
	}

	.btn-primary, .btn-secondary, .btn-danger {
		padding: 10px 20px;
		border: none;
		border-radius: 4px;
		font-weight: 600;
		cursor: pointer;
	}

	.btn-primary {
		background: #4cc9f0;
		color: #0f1419;
	}

	.btn-secondary {
		background: #2c3e50;
		color: #e0e0e0;
	}

	.btn-danger {
		background: #ff6b6b;
		color: #0f1419;
	}

	.btn-text {
		background: transparent;
		border: none;
		color: #4cc9f0;
		cursor: pointer;
	}

	@media (max-width: 640px) {
		.monitor {
			padding: 12px;
		}

		.row {
			flex-direction: column;
			gap: 2px;
		}

		.value {
			word-break: break-word;
		}

		.logs {
			height: 240px;
		}
	}
</style>
