<script>
	import MessagesViewer from './MessagesViewer.svelte'
	import ContactsViewer from './ContactsViewer.svelte'

	export let selectedCase
	export let evidenceAgents = []
	export let summaries = {}
	export let records = {}
	export let activeAgent
	export let onSelectAgent = () => {}
	export let onLoadEvidence
	export let onExport
	export let loading
	export let onOpenHelp = () => {}

	let search = ''
	let limit = 1000
	let viewMode = activeAgent === 'cerberus' ? 'messages' : 'table'
	let previousAgent = activeAgent

	$: if (activeAgent !== previousAgent) {
		previousAgent = activeAgent
		viewMode = activeAgent === 'cerberus' ? 'messages' : 'table'
		search = ''
	}

	$: activeRecords = records[activeAgent] || []
	$: filtered = search
		? activeRecords.filter(r => JSON.stringify(r).toLowerCase().includes(search.toLowerCase()))
		: activeRecords

	$: columns = filtered.length > 0 ? Object.keys(filtered[0]).filter(c => c !== '_id') : []

	$: hasTypedViews = activeAgent === 'cerberus'

	function selectAgent(agent) {
		activeAgent = agent
		onSelectAgent(agent)
		search = ''
		if (agent === 'cerberus') {
			viewMode = 'messages'
		} else {
			viewMode = 'table'
			onLoadEvidence(agent, limit, '')
		}
	}

	function setViewMode(mode) {
		viewMode = mode
		search = ''
		const typeMap = {
			table: '',
			contacts: 'contact'
		}
		if (mode !== 'messages') {
			onLoadEvidence(activeAgent, limit, typeMap[mode] || '')
		}
	}

	function formatCell(value) {
		if (value === null || value === undefined) return ''
		if (typeof value === 'object') return JSON.stringify(value)
		return String(value)
	}
</script>

<div class="explorer">
	<h2>Evidence Explorer</h2>

	{#if !selectedCase}
		<div class="warning">Select a case from the sidebar to browse evidence.</div>
	{:else}
		<div class="layout" class:message-layout={activeAgent === 'cerberus' && viewMode === 'messages'}>
			<aside class="agent-list" class:hidden-in-messages={activeAgent === 'cerberus' && viewMode === 'messages'}>
				<h3>Evidence Sources</h3>
				{#if evidenceAgents.length === 0}
					<p class="hint">No evidence yet. Run an agent first.</p>
				{:else}
					<ul>
						{#each evidenceAgents as agent}
							<li class:active={activeAgent === agent}>
								<button on:click={() => selectAgent(agent)}>
									{agent}
									{#if summaries[agent]}
										<span class="count">({summaries[agent].record_count || 0})</span>
									{/if}
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			</aside>

			<main class="records-panel">
				{#if activeAgent}
					<div class="toolbar">
						<div class="source-heading">
							<h3>{activeAgent}</h3>
							{#if viewMode === 'messages'}
								<select
									class="source-switcher"
									aria-label="Evidence source"
									value={activeAgent}
									on:change={(event) => selectAgent(event.currentTarget.value)}
								>
									{#each evidenceAgents as agent}
										<option value={agent}>{agent}</option>
									{/each}
								</select>
							{/if}
						</div>
						<div class="controls">
							{#if viewMode !== 'messages'}
								<input type="text" bind:value={search} placeholder="Search records..." />
								<select bind:value={limit} on:change={() => setViewMode(viewMode)}>
									<option value={100}>100 rows</option>
									<option value={1000}>1,000 rows</option>
									<option value={10000}>10,000 rows</option>
								</select>
							{/if}
							{#if hasTypedViews}
								<div class="tab-switcher">
									<button class:active={viewMode === 'table'} on:click={() => setViewMode('table')}>All Records</button>
									<button class:active={viewMode === 'messages'} on:click={() => setViewMode('messages')}>Messages</button>
									<button class:active={viewMode === 'contacts'} on:click={() => setViewMode('contacts')}>Contacts</button>
								</div>
							{/if}
							{#if viewMode !== 'messages'}
								<button class="btn-secondary" on:click={() => onExport('csv')}>CSV</button>
								<button class="btn-secondary" on:click={() => onExport('json')}>JSON</button>
								<button class="btn-secondary" on:click={() => onExport('pdf')}>PDF</button>
							{/if}
						</div>
					</div>

					{#if activeAgent === 'cerberus' && viewMode === 'messages'}
						<MessagesViewer
							selectedCase={selectedCase}
							agent={activeAgent}
							onOpenHelp={onOpenHelp}
						/>
					{:else if loading}
						<div class="loading">Loading records...</div>
					{:else if filtered.length === 0}
						<div class="empty">No records match your search.</div>
					{:else if activeAgent === 'cerberus' && viewMode === 'contacts'}
						<ContactsViewer records={activeRecords} onOpenHelp={onOpenHelp} />
					{:else}
						<div class="table-wrap">
							<table>
								<thead>
									<tr>
										{#each columns as col}
											<th>{col}</th>
										{/each}
									</tr>
								</thead>
								<tbody>
									{#each filtered as row}
										<tr>
											{#each columns as col}
												<td title={formatCell(row[col])}>{formatCell(row[col])}</td>
											{/each}
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
						<p class="row-count">Showing {filtered.length} of {activeRecords.length} loaded records.</p>
					{/if}
				{:else}
					<div class="empty">Select an evidence source from the left.</div>
				{/if}
			</main>
		</div>
	{/if}
</div>

<style>
	.explorer {
		padding: 24px;
		height: 100%;
		box-sizing: border-box;
		display: flex;
		flex-direction: column;
	}

	.explorer h2 {
		color: #4cc9f0;
		margin: 0 0 16px;
	}

	.warning {
		background: #3a2a1a;
		border: 1px solid #ff9f1c;
		color: #ff9f1c;
		padding: 12px;
		border-radius: 6px;
	}

	.layout {
		display: flex;
		gap: 20px;
		flex: 1;
		min-height: 0;
	}

	.layout.message-layout {
		gap: 0;
	}

	.agent-list.hidden-in-messages {
		display: none;
	}

	.agent-list {
		width: 220px;
		background: #1a252f;
		border-radius: 8px;
		padding: 16px;
		overflow-y: auto;
		flex-shrink: 0;
	}

	.agent-list h3 {
		color: #4cc9f0;
		margin: 0 0 12px;
		font-size: 1rem;
	}

	.agent-list ul {
		list-style: none;
		padding: 0;
		margin: 0;
	}

	.agent-list li {
		margin-bottom: 4px;
	}

	.agent-list li.active button {
		background: #2c3e50;
		color: #4cc9f0;
	}

	.agent-list button {
		width: 100%;
		text-align: left;
		padding: 10px;
		background: transparent;
		border: none;
		border-radius: 4px;
		color: #a0a0a0;
		cursor: pointer;
	}

	.agent-list button:hover {
		background: #0f1419;
	}

	.count {
		color: #777;
		font-size: 0.8rem;
	}

	.records-panel {
		flex: 1;
		background: #1a252f;
		border-radius: 8px;
		padding: 16px;
		display: flex;
		flex-direction: column;
		min-height: 0;
	}

	.toolbar {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 16px;
		gap: 12px;
		flex-wrap: wrap;
	}

	.toolbar h3 {
		color: #e0e0e0;
		margin: 0;
	}

	.source-heading {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}

	.source-switcher {
		height: 32px;
		max-width: 180px;
		padding: 4px 8px;
		background: #2c3e50;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
	}

	.controls {
		display: flex;
		gap: 10px;
		align-items: center;
		flex-wrap: wrap;
	}

	.controls input {
		padding: 8px 12px;
		background: #2c3e50;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
		min-width: 200px;
	}

	.controls select {
		padding: 8px 12px;
		background: #2c3e50;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
	}

	.table-wrap {
		flex: 1;
		overflow: auto;
		border: 1px solid #2c3e50;
		border-radius: 4px;
	}

	table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.85rem;
	}

	th, td {
		padding: 10px;
		border-bottom: 1px solid #2c3e50;
		text-align: left;
		max-width: 300px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	th {
		background: #0f1419;
		color: #4cc9f0;
		position: sticky;
		top: 0;
	}

	td {
		color: #a0a0a0;
	}

	tr:hover td {
		background: #0f1419;
	}

	.empty, .loading {
		text-align: center;
		padding: 40px;
		color: #777;
	}

	.row-count {
		color: #777;
		font-size: 0.85rem;
		margin-top: 12px;
	}

	.btn-secondary {
		padding: 8px 14px;
		background: #2c3e50;
		border: none;
		border-radius: 4px;
		color: #e0e0e0;
		cursor: pointer;
	}

	.btn-secondary:hover {
		background: #3d4a55;
	}

	.tab-switcher {
		display: flex;
		background: #0f1419;
		border: 1px solid #2c3e50;
		border-radius: 4px;
		overflow: hidden;
	}

	.tab-switcher button {
		padding: 8px 14px;
		background: transparent;
		border: none;
		color: #a0a0a0;
		cursor: pointer;
	}

	.tab-switcher button.active {
		background: #4cc9f0;
		color: #0f1419;
		font-weight: 600;
	}

	.tab-switcher button:hover:not(.active) {
		background: #1a252f;
	}

	@media (max-width: 820px) {
		.explorer {
			height: auto;
			min-height: 100%;
		}

		.layout.message-layout {
			flex: none;
			height: 760px;
			min-height: 760px;
		}

		.layout.message-layout .records-panel {
			height: 100%;
			min-height: 0;
			box-sizing: border-box;
		}
	}

	@media (max-width: 640px) {
		.explorer {
			padding: 12px;
		}

		.layout {
			flex-direction: column;
			gap: 12px;
		}

		.agent-list {
			width: 100%;
			max-height: 180px;
			box-sizing: border-box;
		}

		.agent-list ul {
			display: flex;
			flex-direction: column;
			gap: 2px;
		}

		.records-panel {
			min-height: 300px;
			box-sizing: border-box;
		}

		.toolbar {
			flex-direction: column;
			align-items: stretch;
			gap: 10px;
		}

		.source-heading {
			justify-content: space-between;
		}

		.controls {
			gap: 8px;
		}

		.controls input,
		.controls select {
			min-width: 0;
			flex: 1;
		}

		.tab-switcher {
			width: 100%;
		}

		.tab-switcher button {
			flex: 1;
		}

		.table-wrap {
			max-width: 100vw;
		}
	}
</style>
