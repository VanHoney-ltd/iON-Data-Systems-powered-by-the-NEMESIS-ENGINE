<script>
	export let pulling
	export let status
	export let logs
	export let progress = { overallPercent: 0, filePercent: 0, label: '', active: false }
	export let onPull

	let caseName = ''

	function pull() {
		if (!caseName.trim()) return
		onPull(caseName.trim())
	}
</script>

<div class="acquire">
	<h2>Acquire New iOS Backup</h2>
	<p class="intro">Pull a fresh backup from a connected, trusted iOS device using <code>chronos</code>.</p>

	<div class="panel">
		<div class="form-group">
			<label for="pull-case-name">Case name for this backup</label>
			<input id="pull-case-name" type="text" bind:value={caseName} placeholder="e.g. iPhone-2026-07" />
		</div>

		<div class="tip">
			<p><strong>Before you start:</strong></p>
			<ul>
				<li>Connect your iOS device via USB.</li>
				<li>Unlock the device and tap <strong>Trust This Computer</strong> if prompted.</li>
				<li>Keep the device connected until acquisition finishes.</li>
				<li>If prompted for the encrypted backup password, enter it in the terminal running iON.</li>
			</ul>
		</div>

		<button class="btn-primary" on:click={pull} disabled={pulling || !caseName.trim()}>
			{#if pulling}
				Acquiring...
			{:else}
				Pull Backup from Device
			{/if}
		</button>
	</div>

	{#if status}
		<div class="status-bar"><span class="status-value">{status}</span></div>
	{/if}

	{#if pulling || progress.active || progress.overallPercent > 0}
		<div class="progress-panel">
			<div class="progress-head">
				<span>Backup progress</span>
				<strong>{progress.overallPercent || 0}%</strong>
			</div>
			<div class="progress-track" class:indeterminate={pulling && !progress.overallPercent}>
				<div class="progress-fill" style="width: {progress.overallPercent || 0}%"></div>
			</div>
			<div class="progress-subhead">
				<span>Current file</span>
				<strong>{progress.filePercent || 0}%</strong>
			</div>
			<div class="progress-track small" class:indeterminate={pulling && !progress.filePercent}>
				<div class="progress-fill file" style="width: {progress.filePercent || 0}%"></div>
			</div>
			<div class="progress-label">{progress.label || 'Waiting for idevicebackup2 progress...'}</div>
		</div>
	{/if}

	{#if pulling || logs.length > 0}
		<div class="logs-panel">
			<h3>Acquisition Logs</h3>
			<div class="logs">
				{#each logs as log}
					<div class="log-entry">{log}</div>
				{:else}
					<div class="log-entry empty">Waiting for chronos output...</div>
				{/each}
			</div>
		</div>
	{/if}
</div>

<style>
	.acquire {
		padding: 24px;
		max-width: 800px;
	}

	.acquire h2 {
		color: #4cc9f0;
		margin: 0 0 8px;
	}

	.intro {
		color: #a0a0a0;
		margin-bottom: 24px;
	}

	.intro code {
		background: #2c3e50;
		padding: 2px 6px;
		border-radius: 3px;
		color: #4cc9f0;
	}

	.panel {
		background: #1a252f;
		border-radius: 8px;
		padding: 24px;
		margin-bottom: 20px;
	}

	.form-group {
		margin-bottom: 16px;
	}

	.form-group label {
		display: block;
		color: #a0a0a0;
		margin-bottom: 8px;
		font-weight: 600;
	}

	.form-group input {
		width: 100%;
		padding: 12px;
		background: #2c3e50;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
		font-size: 1rem;
		box-sizing: border-box;
	}

	.tip {
		background: #0f1419;
		border-radius: 6px;
		padding: 16px;
		margin-bottom: 20px;
		color: #a0a0a0;
		font-size: 0.9rem;
	}

	.tip ul {
		margin: 8px 0 0 20px;
	}

	.tip li {
		margin-bottom: 6px;
	}

	.btn-primary {
		padding: 12px 24px;
		background: #4cc9f0;
		color: #0f1419;
		border: none;
		border-radius: 4px;
		font-weight: 600;
		cursor: pointer;
	}

	.btn-primary:disabled {
		background: #555;
		color: #999;
		cursor: not-allowed;
	}

	.status-bar {
		background: #1a252f;
		padding: 12px;
		border-radius: 4px;
		margin-bottom: 16px;
	}

	.status-value {
		color: #ff9f1c;
		font-weight: 600;
	}

	.progress-panel {
		background: #1a252f;
		border: 1px solid #2c3e50;
		border-radius: 8px;
		padding: 16px;
		margin-bottom: 16px;
	}

	.progress-head,
	.progress-subhead {
		display: flex;
		justify-content: space-between;
		align-items: center;
		color: #e0e0e0;
		font-weight: 600;
		margin-bottom: 8px;
	}

	.progress-subhead {
		margin-top: 12px;
		color: #a0a0a0;
		font-size: 0.85rem;
	}

	.progress-track {
		position: relative;
		height: 14px;
		background: #0f1419;
		border: 1px solid #2c3e50;
		border-radius: 999px;
		overflow: hidden;
	}

	.progress-track.small {
		height: 8px;
	}

	.progress-fill {
		height: 100%;
		background: linear-gradient(90deg, #4cc9f0, #80ed99);
		transition: width 0.25s ease;
	}

	.progress-fill.file {
		background: linear-gradient(90deg, #ff9f1c, #f9c74f);
	}

	.progress-track.indeterminate::after {
		content: '';
		position: absolute;
		top: 0;
		bottom: 0;
		width: 35%;
		background: linear-gradient(90deg, transparent, rgba(76, 201, 240, 0.7), transparent);
		animation: progress-scan 1.2s linear infinite;
	}

	.progress-label {
		margin-top: 10px;
		color: #a0a0a0;
		font-size: 0.85rem;
		word-break: break-word;
	}

	@keyframes progress-scan {
		from {
			left: -35%;
		}
		to {
			left: 100%;
		}
	}

	.logs-panel {
		background: #1a252f;
		border-radius: 8px;
		padding: 16px;
	}

	.logs-panel h3 {
		color: #4cc9f0;
		margin: 0 0 12px;
	}

	.logs {
		height: 280px;
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

	@media (max-width: 640px) {
		.acquire {
			padding: 12px;
			max-width: none;
		}

		.panel {
			padding: 16px;
		}

		.logs {
			height: 220px;
		}
	}
</style>
