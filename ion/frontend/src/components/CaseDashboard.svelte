<script>
	export let selectedCase
	export let summary
	export let summaries = {}
	export let onRunAgent
	export let onCreateCase
	export let onViewEvidence

	const quickAgents = [
		{ id: 'vigil', name: 'Vigil', desc: 'Fast system overview', safe: true },
		{ id: 'charon', name: 'Charon', desc: 'Photos and videos', safe: true },
		{ id: 'cerberus', name: 'Cerberus', desc: 'Messages, calls, contacts', safe: true },
		{ id: 'nyx', name: 'Nyx', desc: 'Safari history and bookmarks', safe: true }
	]

	function run(agent) {
		onRunAgent(agent)
	}

	function viewEvidence(agent) {
		if (onViewEvidence) onViewEvidence(agent)
	}

	function agentCount(summary) {
		if (!summary || !summary.evidenceAgents) return 0
		return summary.evidenceAgents.length
	}
</script>

<div class="dashboard">
	{#if !selectedCase}
		<div class="empty-state">
			<h2>No case selected</h2>
			<p>Select an existing case from the menu, or create a new one to get started.</p>
			<button class="btn-primary" on:click={onCreateCase}>Create New Case</button>
		</div>
	{:else if !summary}
		<div class="loading">Loading case summary...</div>
	{:else}
		<div class="header">
			<div>
				<h2>{summary.name}</h2>
				<p class="path" title={summary.rootPath}>{summary.rootPath}</p>
			</div>
			<div class="meta">
				<span class="badge" class:ok={summary.hasBackup}>{summary.hasBackup ? 'Backup linked' : 'No backup'}</span>
				<span class="badge">{summary.backupSize}</span>
				<span class="badge">{summary.lastRunStatus}</span>
			</div>
		</div>

		<div class="stats">
			<div class="stat-card">
				<span class="stat-value">{agentCount(summary)}</span>
				<span class="stat-label">Evidence sources</span>
			</div>
			<div class="stat-card">
				<span class="stat-value">{summary.backupSize}</span>
				<span class="stat-label">Backup size</span>
			</div>
			<div class="stat-card">
				<span class="stat-value">{summary.hasBackup ? 'Yes' : 'No'}</span>
				<span class="stat-label">Backup linked</span>
			</div>
		</div>

		<div class="grid">
			<div class="card evidence-card">
				<h3>Evidence Sources</h3>
				{#if summary.evidenceAgents.length === 0}
					<p class="hint">No agents have been run yet. Run a quick action below to generate evidence.</p>
				{:else}
					<div class="agent-grid">
						{#each summary.evidenceAgents as agent}
							<button class="agent-tile" on:click={() => viewEvidence(agent)}>
								<span class="agent-name">{agent}</span>
								{#if summaries[agent]}
									<span class="agent-count">{summaries[agent].record_count.toLocaleString()} records</span>
								{/if}
							</button>
						{/each}
					</div>
				{/if}
				<button class="btn-secondary" on:click={() => viewEvidence()}>Browse All Evidence</button>
			</div>

			<div class="card">
				<h3>Backup Location</h3>
				<p class="path" title={summary.backupPath}>{summary.backupPath}</p>
				<p class="hint">This is a symlink to your original iOS backup. If it is missing, use Decrypt Backup or Acquire Backup to refresh it.</p>
			</div>
		</div>

		<div class="quick-actions">
			<h3>Quick Actions</h3>
			<div class="action-grid">
				{#each quickAgents as agent}
					<button class="btn-action" on:click={() => run(agent.id)} title={agent.desc}>
						<span class="action-name">{agent.name}</span>
						<span class="action-desc">{agent.desc}</span>
					</button>
				{/each}
			</div>
			<p class="tip">These are safe starting agents. For a 70 GB+ backup, begin with Vigil or Nyx before running Charon or Hermes.</p>
		</div>
	{/if}
</div>

<style>
	.dashboard {
		padding: 24px;
	}

	.empty-state {
		text-align: center;
		padding: 60px 20px;
		background: #1a252f;
		border-radius: 8px;
	}

	.empty-state h2 {
		color: #4cc9f0;
		margin-bottom: 12px;
	}

	.empty-state p {
		color: #a0a0a0;
		margin-bottom: 24px;
	}

	.loading {
		text-align: center;
		padding: 60px;
		color: #777;
	}

	.header {
		display: flex;
		justify-content: space-between;
		align-items: flex-start;
		margin-bottom: 24px;
		flex-wrap: wrap;
		gap: 12px;
	}

	.header h2 {
		color: #4cc9f0;
		margin: 0 0 4px;
	}

	.path {
		color: #777;
		font-family: monospace;
		font-size: 0.8rem;
		margin: 0;
		word-break: break-all;
	}

	.meta {
		display: flex;
		gap: 10px;
		flex-wrap: wrap;
	}

	.badge {
		background: #2c3e50;
		color: #a0a0a0;
		padding: 6px 12px;
		border-radius: 12px;
		font-size: 0.85rem;
	}

	.badge.ok {
		background: #1e3a3a;
		color: #2ec4b6;
	}

	.stats {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
		gap: 16px;
		margin-bottom: 24px;
	}

	.stat-card {
		background: #1a252f;
		border-radius: 8px;
		padding: 20px;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.stat-value {
		color: #4cc9f0;
		font-size: 1.6rem;
		font-weight: 700;
	}

	.stat-label {
		color: #777;
		font-size: 0.85rem;
	}

	.grid {
		display: grid;
		grid-template-columns: 2fr 1fr;
		gap: 20px;
		margin-bottom: 24px;
	}

	@media (max-width: 900px) {
		.grid {
			grid-template-columns: 1fr;
		}
	}

	.card {
		background: #1a252f;
		border-radius: 8px;
		padding: 20px;
	}

	.card h3 {
		color: #4cc9f0;
		margin: 0 0 16px;
	}

	.hint {
		color: #777;
		font-size: 0.85rem;
		margin: 0 0 16px;
	}

	.agent-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
		gap: 12px;
		margin-bottom: 16px;
	}

	.agent-tile {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 14px;
		background: #0f1419;
		border: 1px solid #2c3e50;
		border-radius: 6px;
		color: #e0e0e0;
		cursor: pointer;
		text-align: left;
		transition: all 0.2s;
	}

	.agent-tile:hover {
		border-color: #4cc9f0;
		background: #1a252f;
	}

	.agent-name {
		font-weight: 600;
		text-transform: capitalize;
	}

	.agent-count {
		color: #777;
		font-size: 0.8rem;
	}

	.quick-actions {
		background: #1a252f;
		border-radius: 8px;
		padding: 20px;
	}

	.quick-actions h3 {
		color: #4cc9f0;
		margin: 0 0 16px;
	}

	.action-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
		gap: 12px;
		margin-bottom: 12px;
	}

	.btn-action {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 16px;
		background: #2c3e50;
		border: 1px solid #3d4a55;
		border-radius: 6px;
		color: #e0e0e0;
		cursor: pointer;
		transition: all 0.2s;
		text-align: left;
	}

	.btn-action:hover {
		background: #3d4a55;
		border-color: #4cc9f0;
	}

	.action-name {
		font-weight: 600;
		font-size: 1rem;
	}

	.action-desc {
		font-size: 0.8rem;
		color: #a0a0a0;
	}

	.tip {
		color: #777;
		font-size: 0.85rem;
		margin: 0;
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

	.btn-secondary {
		background: #2c3e50;
		color: #e0e0e0;
	}

	.btn-secondary:hover {
		background: #3d4a55;
	}

	@media (max-width: 640px) {
		.dashboard {
			padding: 12px;
		}

		.header {
			flex-direction: column;
		}

		.stats {
			grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
			gap: 10px;
		}

		.stat-card {
			padding: 14px;
		}

		.stat-value {
			font-size: 1.3rem;
		}

		.agent-grid,
		.action-grid {
			grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
		}
	}
</style>
