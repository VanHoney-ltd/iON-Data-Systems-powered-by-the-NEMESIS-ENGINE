<script>
	export let onCreated
	export let onCancel

	import { SelectBackupDirectory, CreateCaseFromBackup } from '../../wailsjs/go/main/App.js'

	let step = 1
	let caseName = ''
	let backupPath = ''
	let status = ''
	let creating = false

	async function browse() {
		try {
			const path = await SelectBackupDirectory()
			if (path) backupPath = path
		} catch (e) {
			status = `Could not open directory picker: ${e}`
		}
	}

	async function create() {
		if (!caseName.trim() || !backupPath) return
		creating = true
		status = 'Creating case...'
		try {
			const created = await CreateCaseFromBackup(caseName.trim(), backupPath)
			status = `Created case "${created}".`
			onCreated(created)
		} catch (e) {
			status = `Error: ${e}`
		} finally {
			creating = false
		}
	}

	function next() {
		if (step === 1 && caseName.trim()) step = 2
	}

	function back() {
		if (step > 1) step--
	}
</script>

<div class="wizard">
	<div class="wizard-header">
		<h2>Create New Case</h2>
		<p>Point iON at an iOS backup folder. The backup is symlinked, not copied.</p>
	</div>

	<div class="steps">
		<div class="step-indicator">
			<span class:active={step === 1}>1. Name</span>
			<span class:active={step === 2}>2. Backup</span>
			<span class:active={step === 3}>3. Confirm</span>
		</div>

		{#if step === 1}
			<div class="step">
				<label for="case-name">Case name</label>
				<input id="case-name" type="text" bind:value={caseName} placeholder="e.g. Iowa-2026" />
				<p class="tip">Pick a short, memorable name. You can use letters, numbers, dashes, and underscores.</p>
			</div>
		{:else if step === 2}
			<div class="step">
				<label for="case-backup-path">Backup directory</label>
				<div class="path-row">
					<input id="case-backup-path" type="text" readonly value={backupPath || '(none selected)'} />
					<button class="btn-secondary" on:click={browse}>Browse...</button>
				</div>
				<p class="tip">Select the folder containing <code>Manifest.db</code> or <code>Manifest.plist</code>. This is usually a folder with a long hexadecimal name inside your iTunes/iMazing backup location.</p>
			</div>
		{:else if step === 3}
			<div class="step">
				<h3>Review</h3>
				<ul class="review-list">
					<li><strong>Case name:</strong> {caseName}</li>
					<li><strong>Backup path:</strong> {backupPath}</li>
				</ul>
				<p class="tip">iON will create a symlink to this backup. Your original files will not be moved or copied.</p>
			</div>
		{/if}
	</div>

	{#if status}
		<div class="status-bar"><span class="status-value">{status}</span></div>
	{/if}

	<div class="actions">
		{#if step > 1}
			<button class="btn-secondary" on:click={back}>Back</button>
		{:else}
			<button class="btn-secondary" on:click={onCancel}>Cancel</button>
		{/if}

		{#if step < 3}
			<button class="btn-primary" on:click={next} disabled={step === 1 && !caseName.trim()}>Next</button>
		{:else}
			<button class="btn-primary" on:click={create} disabled={creating || !backupPath}>Create Case</button>
		{/if}
	</div>
</div>

<style>
	.wizard {
		max-width: 640px;
		background: #1a252f;
		border-radius: 8px;
		padding: 30px;
		margin: 0 auto;
	}

	.wizard-header h2 {
		color: #4cc9f0;
		margin: 0 0 8px;
	}

	.wizard-header p {
		color: #a0a0a0;
		margin: 0 0 24px;
	}

	.steps {
		margin-bottom: 24px;
	}

	.step-indicator {
		display: flex;
		gap: 16px;
		margin-bottom: 24px;
		color: #777;
		font-size: 0.85rem;
	}

	.step-indicator span.active {
		color: #4cc9f0;
		font-weight: 600;
	}

	.step label {
		display: block;
		color: #a0a0a0;
		margin-bottom: 8px;
		font-weight: 600;
	}

	.step input {
		width: 100%;
		padding: 12px;
		background: #2c3e50;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
		font-size: 1rem;
		box-sizing: border-box;
	}

	.path-row {
		display: flex;
		gap: 10px;
	}

	.path-row input {
		flex: 1;
	}

	.tip {
		color: #777;
		font-size: 0.85rem;
		margin-top: 12px;
		line-height: 1.4;
	}

	.tip code {
		background: #2c3e50;
		padding: 2px 6px;
		border-radius: 3px;
		color: #4cc9f0;
	}

	.review-list {
		background: #0f1419;
		border-radius: 6px;
		padding: 16px;
		list-style: none;
		margin: 0 0 16px;
	}

	.review-list li {
		margin-bottom: 8px;
		color: #e0e0e0;
	}

	.actions {
		display: flex;
		justify-content: flex-end;
		gap: 12px;
		margin-top: 24px;
	}

	.btn-primary, .btn-secondary {
		padding: 12px 24px;
		border: none;
		border-radius: 4px;
		font-size: 0.95rem;
		font-weight: 600;
		cursor: pointer;
	}

	.btn-primary {
		background: #4cc9f0;
		color: #0f1419;
	}

	.btn-primary:disabled {
		background: #555;
		color: #999;
		cursor: not-allowed;
	}

	.btn-secondary {
		background: #2c3e50;
		color: #e0e0e0;
	}

	.status-bar {
		background: #0f1419;
		padding: 12px;
		border-radius: 4px;
		margin-bottom: 16px;
	}

	.status-value {
		color: #ff9f1c;
	}

	@media (max-width: 640px) {
		.wizard {
			padding: 20px;
			margin: 12px;
		}

		.path-row {
			flex-direction: column;
		}

		.actions {
			justify-content: stretch;
		}

		.actions button {
			flex: 1;
		}
	}
</style>
