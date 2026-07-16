<script>
	export let agents = []
	export let selectedCase
	export let onRun
	export let onOpenHelp
	export let backupPassword = ''

	let expanded = null

	function toggle(id) {
		expanded = expanded === id ? null : id
	}
</script>

<div class="catalog">
	<h2>Agent Catalog</h2>
	<p class="intro">Choose an agent to extract data from the active case. Hover over a card for details, or click <strong>Learn more</strong>.</p>

	{#if !selectedCase}
		<div class="warning">Select a case from the sidebar before running an agent.</div>
	{/if}

	<div class="password-row">
		<label for="backup-password">Backup password (only if encrypted):</label>
		<input
			id="backup-password"
			type="password"
			bind:value={backupPassword}
			placeholder="Leave blank for unencrypted backups"
		/>
		<button class="btn-text" on:click={() => onOpenHelp('encrypted')}>Why do I need this?</button>
	</div>

	<div class="grid">
		{#each agents as agent}
			<div class="agent-card" class:disabled={!selectedCase}>
				<div class="card-header">
					<h3>{agent.name}</h3>
					<span class="runtime">{agent.estimatedTime}</span>
				</div>
				<p class="description">{agent.description}</p>
				<p class="scope">{agent.scope}</p>

				<div class="tags">
					<span class="tag risk" title="Risk/scope level">{agent.risk}</span>
					{#if agent.requiresDecryption}
						<span class="tag lock" title="Requires decrypted backup or password">Encrypted OK</span>
					{/if}
				</div>

				{#if expanded === agent.id}
					<div class="details">
						<p><strong>Tip:</strong> {agent.tip}</p>
					</div>
				{/if}

				<div class="actions">
					<button
						class="btn-primary"
						disabled={!selectedCase}
						on:click={() => onRun(agent.id)}
						title={selectedCase ? `Run ${agent.name}` : 'Select a case first'}
					>
						Run {agent.name}
					</button>
					<button class="btn-text" on:click={() => toggle(agent.id)}>
						{expanded === agent.id ? 'Less' : 'Learn more'}
					</button>
					<button class="btn-text" on:click={() => onOpenHelp(agent.id)}>Help</button>
				</div>
			</div>
		{/each}
	</div>
</div>

<style>
	.catalog {
		padding: 24px;
	}

	.catalog h2 {
		color: #4cc9f0;
		margin: 0 0 8px;
	}

	.intro {
		color: #a0a0a0;
		margin-bottom: 12px;
	}

	.password-row {
		display: flex;
		align-items: center;
		gap: 12px;
		margin-bottom: 24px;
		flex-wrap: wrap;
	}

	.password-row label {
		color: #a0a0a0;
		font-size: 0.9rem;
	}

	.password-row input {
		padding: 10px 12px;
		background: #2c3e50;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
		min-width: 220px;
	}

	.warning {
		background: #3a2a1a;
		border: 1px solid #ff9f1c;
		color: #ff9f1c;
		padding: 12px;
		border-radius: 6px;
		margin-bottom: 24px;
	}

	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
		gap: 20px;
	}

	.agent-card {
		background: #1a252f;
		border-radius: 8px;
		padding: 20px;
		border: 1px solid transparent;
		transition: border-color 0.2s;
	}

	.agent-card:hover {
		border-color: #3d4a55;
	}

	.agent-card.disabled {
		opacity: 0.7;
	}

	.card-header {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		margin-bottom: 12px;
	}

	.card-header h3 {
		color: #e0e0e0;
		margin: 0;
	}

	.runtime {
		color: #777;
		font-size: 0.8rem;
	}

	.description {
		color: #4cc9f0;
		font-weight: 600;
		margin: 0 0 8px;
	}

	.scope {
		color: #a0a0a0;
		font-size: 0.9rem;
		margin: 0 0 16px;
	}

	.tags {
		display: flex;
		gap: 8px;
		margin-bottom: 16px;
		flex-wrap: wrap;
	}

	.tag {
		font-size: 0.75rem;
		padding: 4px 8px;
		border-radius: 12px;
		background: #2c3e50;
		color: #a0a0a0;
	}

	.tag.risk {
		background: #2a1a1a;
		color: #ff9f1c;
	}

	.tag.lock {
		background: #1a2a1a;
		color: #2ec4b6;
	}

	.details {
		background: #0f1419;
		border-radius: 6px;
		padding: 12px;
		margin-bottom: 16px;
		color: #a0a0a0;
		font-size: 0.9rem;
	}

	.actions {
		display: flex;
		gap: 10px;
		align-items: center;
	}

	.btn-primary {
		padding: 10px 18px;
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

	.btn-text {
		background: transparent;
		border: none;
		color: #4cc9f0;
		cursor: pointer;
		font-size: 0.85rem;
	}

	.btn-text:hover {
		text-decoration: underline;
	}

	@media (max-width: 640px) {
		.catalog {
			padding: 12px;
		}

		.password-row {
			flex-direction: column;
			align-items: stretch;
		}

		.password-row input {
			min-width: 0;
			width: 100%;
		}

		.grid {
			grid-template-columns: 1fr;
		}

		.card-header {
			flex-direction: column;
			align-items: flex-start;
			gap: 4px;
		}

		.actions {
			flex-wrap: wrap;
		}
	}
</style>
