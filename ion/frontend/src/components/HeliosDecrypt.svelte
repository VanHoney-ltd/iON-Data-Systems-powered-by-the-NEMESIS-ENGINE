<script>
	export let pulling
	export let status
	export let logs
	export let onDecrypt

	import { SelectBackupDirectory } from '../../wailsjs/go/main/App.js'

	let caseName = ''
	let backupPath = ''
	let password = ''
	let profile = 'full'

	const profiles = [
		'full',
		'messages',
		'contacts',
		'calls',
		'voicemail',
		'mail',
		'location',
		'photos',
		'appdata',
		'finance'
	]

	async function browse() {
		const path = await SelectBackupDirectory()
		if (path) backupPath = path
	}

	function decrypt() {
		if (!caseName.trim() || !backupPath || !password) return
		onDecrypt(caseName.trim(), backupPath, password, profile)
	}
</script>

<div class="decrypt">
	<h2>Decrypt Backup with HELiOS</h2>
	<p class="intro">Decrypt an encrypted iOS backup into a prepared folder that agents can read.</p>

	<div class="panel">
		<div class="form-group">
			<label for="helios-case-name">Case name</label>
			<input id="helios-case-name" type="text" bind:value={caseName} placeholder="e.g. Iowa-2026" />
		</div>

		<div class="form-group">
			<label for="helios-backup-path">Encrypted backup directory</label>
			<div class="path-row">
				<input id="helios-backup-path" type="text" readonly value={backupPath || '(none selected)'} />
				<button class="btn-secondary" on:click={browse}>Browse...</button>
			</div>
		</div>

		<div class="form-group">
			<label for="helios-password">Backup password</label>
			<input id="helios-password" type="password" bind:value={password} placeholder="Required for encrypted backups" />
		</div>

		<div class="form-group">
			<label for="helios-profile">Extraction profile</label>
			<select id="helios-profile" bind:value={profile}>
				{#each profiles as p}
					<option value={p}>{p}</option>
				{/each}
			</select>
			<p class="hint">Full decrypts everything. Use a profile to save time and disk space if you only need messages, photos, etc.</p>
		</div>

		<button class="btn-primary" on:click={decrypt} disabled={pulling || !caseName.trim() || !backupPath || !password}>
			{#if pulling}
				Decrypting...
			{:else}
				Decrypt Backup
			{/if}
		</button>
	</div>

	{#if status}
		<div class="status-bar"><span class="status-value">{status}</span></div>
	{/if}

	{#if pulling || logs.length > 0}
		<div class="logs-panel">
			<h3>HELiOS Logs</h3>
			<div class="logs">
				{#each logs as log}
					<div class="log-entry">{log}</div>
				{:else}
					<div class="log-entry empty">Waiting for HELiOS output...</div>
				{/each}
			</div>
		</div>
	{/if}
</div>

<style>
	.decrypt {
		padding: 24px;
		max-width: 800px;
	}

	.decrypt h2 {
		color: #4cc9f0;
		margin: 0 0 8px;
	}

	.intro {
		color: #a0a0a0;
		margin-bottom: 24px;
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

	.form-group input,
	.form-group select {
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

	.hint {
		color: #777;
		font-size: 0.8rem;
		margin-top: 8px;
	}

	.btn-primary, .btn-secondary {
		padding: 12px 24px;
		border: none;
		border-radius: 4px;
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
		background: #1a252f;
		padding: 12px;
		border-radius: 4px;
		margin-bottom: 16px;
	}

	.status-value {
		color: #ff9f1c;
		font-weight: 600;
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
		.decrypt {
			padding: 12px;
			max-width: none;
		}

		.panel {
			padding: 16px;
		}

		.path-row {
			flex-direction: column;
		}

		.logs {
			height: 220px;
		}
	}
</style>
