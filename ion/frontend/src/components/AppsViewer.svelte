<script>
	import { GetInstalledApps, OpenAppContainer, SearchAppData, ListAppFiles, ReadAppFile } from '../../wailsjs/go/main/App.js'
	import { onMount } from 'svelte'

	export let selectedCase = ''
	export let onOpenHelp = () => {}

	let apps = []
	let loading = false
	let error = null
	let search = ''
	let sortBy = 'size'
	let selectedApp = null

	// File browser state
	let currentPath = ''
	let files = []
	let fileLoading = false
	let fileError = null
	let previewFile = null
	let previewContent = ''
	let previewLoading = false
	let previewError = null

	// Search state
	let showSearch = false
	let appDataQuery = ''
	let appDataHits = []
	let appDataLoading = false
	let appDataError = null
	let showAppList = true

	$: filtered = apps
		.filter(a => {
			if (!search) return true
			const term = search.toLowerCase()
			return (a.name || '').toLowerCase().includes(term) ||
				(a.bundle_id || '').toLowerCase().includes(term) ||
				(a.domain || '').toLowerCase().includes(term)
		})
		.sort((a, b) => {
			if (sortBy === 'size') return b.data_size_bytes - a.data_size_bytes
			if (sortBy === 'name') return (a.name || '').localeCompare(b.name || '')
			if (sortBy === 'files') return b.file_count - a.file_count
			return 0
		})

	$: breadcrumbs = currentPath ? currentPath.split('/').filter(Boolean) : []

	async function loadApps() {
		if (!selectedCase) {
			error = 'Select a case first'
			return
		}
		loading = true
		error = null
		try {
			apps = await GetInstalledApps(selectedCase)
			selectedApp = null
			currentPath = ''
			files = []
			previewFile = null
		} catch (e) {
			error = String(e)
			apps = []
		} finally {
			loading = false
		}
	}

	function selectApp(app) {
		selectedApp = app
		currentPath = app.container_path || ''
		files = []
		previewFile = null
		previewContent = ''
		showAppList = false
		if (currentPath) loadFiles(currentPath)
	}

	function backToAppList() {
		showAppList = true
		selectedApp = null
	}

	async function loadFiles(path) {
		if (!path) return
		fileLoading = true
		fileError = null
		try {
			files = await ListAppFiles(path)
			files.sort((a, b) => {
				if (a.dir !== b.dir) return a.dir ? -1 : 1
				return a.name.localeCompare(b.name)
			})
		} catch (e) {
			fileError = String(e)
			files = []
		} finally {
			fileLoading = false
		}
	}

	function enterDir(path) {
		currentPath = path
		loadFiles(path)
		previewFile = null
		previewContent = ''
	}

	function goUp() {
		if (!currentPath) return
		const parent = currentPath.split('/').slice(0, -1).join('/') || '/'
		enterDir(parent === '/' ? '' : parent)
	}

	function jumpToBreadcrumb(index) {
		const parts = currentPath.split('/').filter(Boolean)
		const target = parts.slice(0, index + 1).join('/')
		enterDir('/' + target)
	}

	function openContainer(path) {
		if (!path) return
		OpenAppContainer(path).catch(e => alert(`Could not open container: ${e}`))
	}

	async function previewSelected(file) {
		if (file.dir) {
			enterDir(file.path)
			return
		}
		if (file.size > 256 * 1024) {
			previewFile = file
			previewContent = ''
			previewError = 'File is too large to preview. Open it externally.'
			return
		}
		previewLoading = true
		previewError = null
		previewFile = file
		previewContent = ''
		try {
			previewContent = await ReadAppFile(file.path)
		} catch (e) {
			previewError = `Could not read file: ${e}`
		} finally {
			previewLoading = false
		}
	}

	async function searchAppData() {
		if (!appDataQuery.trim() || !selectedCase) return
		appDataLoading = true
		appDataError = null
		try {
			appDataHits = await SearchAppData(selectedCase, appDataQuery.trim())
		} catch (e) {
			appDataError = String(e)
			appDataHits = []
		} finally {
			appDataLoading = false
		}
	}

	function humanSize(bytes) {
		if (bytes === 0) return '0 B'
		const units = ['B', 'KB', 'MB', 'GB', 'TB']
		const i = Math.floor(Math.log(bytes) / Math.log(1024))
		return `${(bytes / Math.pow(1024, i)).toFixed(2)} ${units[i]}`
	}

	onMount(() => {
		loadApps()
	})
</script>

<div class="apps-viewer">
	<div class="toolbar">
		<h2>Installed Apps</h2>
		<div class="controls">
			<input type="text" bind:value={search} placeholder="Search apps..." />
			<select bind:value={sortBy}>
				<option value="size">Sort by size</option>
				<option value="name">Sort by name</option>
				<option value="files">Sort by file count</option>
			</select>
			<button class="btn-secondary" on:click={loadApps} disabled={loading}>
				{loading ? 'Loading...' : 'Refresh'}
			</button>
			<button class="btn-text" on:click={() => onOpenHelp('apps')}>About apps</button>
		</div>
	</div>

	{#if error}
		<div class="error">{error}</div>
	{/if}

	<div class="split" class:list-only={showAppList} class:detail-only={!showAppList}>
		<aside class="app-list" class:hidden={!showAppList}>
			<div class="list-header">{filtered.length} app{filtered.length === 1 ? '' : 's'}</div>
			{#if loading && apps.length === 0}
				<div class="empty">Loading apps...</div>
			{:else if filtered.length === 0}
				<div class="empty">No apps found.</div>
			{:else}
				{#each filtered as app}
					<button
						class="app-item"
						class:active={selectedApp === app}
						on:click={() => selectApp(app)}
					>
						<div class="app-main">
							<span class="app-name">{app.name || app.bundle_id}</span>
							<span class="app-size">{app.data_size}</span>
						</div>
						<div class="app-sub">{app.bundle_id}</div>
					</button>
				{/each}
			{/if}
		</aside>

		<main class="app-pane" class:hidden={showAppList}>
			{#if selectedApp}
				<div class="app-card">
					<button class="btn-back" on:click={backToAppList}>← Back to apps</button>
					<h3>{selectedApp.name || selectedApp.bundle_id}</h3>
					<div class="detail-grid">
						<div class="detail-item">
							<span class="label">Bundle ID</span>
							<span class="value">{selectedApp.bundle_id}</span>
						</div>
						<div class="detail-item">
							<span class="label">Domain</span>
							<span class="value">{selectedApp.domain}</span>
						</div>
						<div class="detail-item">
							<span class="label">Data size</span>
							<span class="value">{selectedApp.data_size} ({selectedApp.data_size_bytes.toLocaleString()} bytes)</span>
						</div>
						<div class="detail-item">
							<span class="label">Files</span>
							<span class="value">{selectedApp.file_count.toLocaleString()}</span>
						</div>
						<div class="detail-item">
							<span class="label">Documents</span>
							<span class="value">{selectedApp.has_documents ? 'Yes' : 'No'}</span>
						</div>
						<div class="detail-item">
							<span class="label">Library</span>
							<span class="value">{selectedApp.has_library ? 'Yes' : 'No'}</span>
						</div>
						<div class="detail-item">
							<span class="label">Source</span>
							<span class="value">{selectedApp.source}</span>
						</div>
					</div>

					{#if selectedApp.group_containers && selectedApp.group_containers.length > 0}
						<div class="group-containers">
							<h4>Shared group containers</h4>
							<ul>
								{#each selectedApp.group_containers as gc}
									<li>{gc}</li>
								{/each}
							</ul>
						</div>
					{/if}

					{#if selectedApp.container_path}
						<div class="actions">
							<button class="btn-primary" on:click={() => openContainer(selectedApp.container_path)}>
								Open in File Manager
							</button>
							<button class="btn-secondary" on:click={() => loadFiles(currentPath)} disabled={fileLoading}>
								{fileLoading ? 'Loading...' : 'Refresh Files'}
							</button>
							<button class="btn-secondary" on:click={() => showSearch = !showSearch}>
								{showSearch ? 'Hide Search' : 'Search App Data'}
							</button>
						</div>
					{/if}
				</div>

				{#if selectedApp.container_path}
					<div class="file-browser">
						<div class="browser-header">
							<div class="breadcrumbs">
								<button class="crumb" on:click={() => enterDir(selectedApp.container_path)}>📁 {selectedApp.name || selectedApp.bundle_id}</button>
								{#each breadcrumbs as crumb, i}
									<span class="crumb-sep">/</span>
									{#if i < breadcrumbs.length - 1}
										<button class="crumb" on:click={() => jumpToBreadcrumb(i)}>{crumb}</button>
									{:else}
										<span class="crumb current">{crumb}</span>
									{/if}
								{/each}
							</div>
							{#if currentPath && currentPath !== selectedApp.container_path}
								<button class="btn-text" on:click={goUp}>⬆ Up</button>
							{/if}
						</div>

						{#if fileError}
							<div class="error">{fileError}</div>
						{/if}

						<div class="file-list">
							{#if fileLoading && files.length === 0}
								<div class="empty">Loading files...</div>
							{:else if files.length === 0}
								<div class="empty">No files in this folder.</div>
							{:else}
								<div class="file-list-header">
									<span class="col-name">Name</span>
									<span class="col-size">Size</span>
								</div>
								{#each files as file}
									<button
										class="file-row"
										on:click={() => previewSelected(file)}
										title={file.path}
									>
										<span class="file-icon">{file.dir ? '📁' : '📄'}</span>
										<span class="file-name">{file.name}</span>
										<span class="file-size">{file.dir ? '—' : humanSize(file.size)}</span>
									</button>
								{/each}
							{/if}
						</div>
					</div>
				{/if}

				{#if showSearch}
					<div class="search-panel">
						<h3>Search App Data</h3>
						<div class="search-controls">
							<input type="text" bind:value={appDataQuery} placeholder="Phone, keyword, username..." on:keydown={(e) => e.key === 'Enter' && searchAppData()} />
							<button class="btn-secondary" on:click={searchAppData} disabled={appDataLoading}>
								{appDataLoading ? 'Searching...' : 'Search'}
							</button>
							<button class="btn-text" on:click={() => showSearch = false}>Close</button>
						</div>
						{#if appDataError}
							<div class="error">{appDataError}</div>
						{/if}
						{#if appDataHits.length > 0}
							<div class="hits-header">{appDataHits.length} hit{appDataHits.length === 1 ? '' : 's'}</div>
							<div class="hits-list">
								{#each appDataHits as hit}
									<div class="hit-item">
										<div class="hit-app">{hit.app_name || hit.bundle_id}</div>
										<div class="hit-path" title={hit.file_path}>{hit.file_path}</div>
										<div class="hit-snippet">{hit.snippet}</div>
									</div>
								{/each}
							</div>
						{:else if !appDataLoading && appDataQuery && !appDataError}
							<div class="empty">No hits found.</div>
						{/if}
					</div>
				{/if}

				{#if previewFile}
					<div class="preview-panel">
						<div class="preview-header">
							<h4>Preview: {previewFile.name}</h4>
							<div class="preview-actions">
								<button class="btn-secondary" on:click={() => openContainer(previewFile.path)}>Open Externally</button>
								<button class="btn-text" on:click={() => { previewFile = null; previewContent = ''; previewError = null }}>Close</button>
							</div>
						</div>
						{#if previewLoading}
							<div class="empty">Loading preview...</div>
						{:else if previewError}
							<div class="error">{previewError}</div>
						{:else}
							<pre class="preview-content">{previewContent}</pre>
						{/if}
					</div>
				{/if}
		{:else}
			<div class="empty">Select an app to browse its container.</div>
		{/if}
		</main>
	</div>
</div>

<style>
	.apps-viewer {
		padding: 24px;
		height: calc(100vh - 48px);
		display: flex;
		flex-direction: column;
		min-height: 0;
	}

	.toolbar {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 20px;
		gap: 12px;
		flex-wrap: wrap;
	}

	.toolbar h2 {
		color: #4cc9f0;
		margin: 0;
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

	.error {
		background: #2a1a1a;
		color: #ff6b6b;
		padding: 12px;
		border-radius: 6px;
		margin-bottom: 16px;
	}

	.split {
		display: flex;
		flex: 1;
		gap: 16px;
		min-height: 0;
	}

	.app-list {
		width: 320px;
		overflow-y: auto;
		background: #0f1419;
		border: 1px solid #2c3e50;
		border-radius: 4px;
		padding: 8px;
		flex-shrink: 0;
	}

	.list-header {
		padding: 8px;
		color: #777;
		font-size: 0.8rem;
		border-bottom: 1px solid #2c3e50;
		margin-bottom: 4px;
	}

	.app-item {
		width: 100%;
		text-align: left;
		padding: 12px;
		background: transparent;
		border: none;
		border-bottom: 1px solid #2c3e50;
		color: #e0e0e0;
		cursor: pointer;
	}

	.app-item:hover,
	.app-item.active {
		background: #1a252f;
	}

	.app-main {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 8px;
		margin-bottom: 4px;
	}

	.app-name {
		font-weight: 600;
		word-break: break-word;
		flex: 1;
	}

	.app-size {
		color: #4cc9f0;
		font-size: 0.85rem;
		white-space: nowrap;
	}

	.app-sub {
		font-size: 0.75rem;
		color: #777;
		word-break: break-all;
	}

	.app-pane {
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 16px;
		overflow-y: auto;
		background: #0f1419;
		border: 1px solid #2c3e50;
		border-radius: 4px;
		padding: 20px;
		min-height: 0;
	}

	.app-card h3 {
		color: #4cc9f0;
		margin: 0 0 16px;
	}

	.detail-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
		gap: 16px;
		margin-bottom: 20px;
	}

	.detail-item {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.detail-item .label {
		font-size: 0.7rem;
		color: #777;
		text-transform: uppercase;
		letter-spacing: 0.5px;
	}

	.detail-item .value {
		color: #e0e0e0;
		font-size: 0.9rem;
		word-break: break-word;
	}

	.group-containers {
		margin-bottom: 20px;
		padding-top: 16px;
		border-top: 1px solid #2c3e50;
	}

	.group-containers h4 {
		color: #4cc9f0;
		margin: 0 0 10px;
	}

	.group-containers ul {
		list-style: none;
		padding: 0;
		margin: 0;
	}

	.group-containers li {
		padding: 6px 0;
		color: #a0a0a0;
		border-bottom: 1px solid #1a252f;
		word-break: break-all;
	}

	.actions {
		display: flex;
		gap: 10px;
		flex-wrap: wrap;
	}

	.btn-primary, .btn-secondary {
		padding: 10px 18px;
		border: none;
		border-radius: 4px;
		cursor: pointer;
		font-weight: 600;
	}

	.btn-primary {
		background: #4cc9f0;
		color: #0f1419;
	}

	.btn-primary:hover:not(:disabled) {
		background: #3bb8df;
	}

	.btn-secondary {
		background: #2c3e50;
		color: #e0e0e0;
	}

	.btn-secondary:hover:not(:disabled) {
		background: #3d4a55;
	}

	button:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}

	.empty {
		text-align: center;
		padding: 40px;
		color: #777;
	}

	.btn-text {
		background: transparent;
		border: none;
		color: #4cc9f0;
		cursor: pointer;
		padding: 6px 10px;
	}

	.file-browser {
		background: #0a0e12;
		border: 1px solid #2c3e50;
		border-radius: 4px;
		display: flex;
		flex-direction: column;
		min-height: 260px;
		max-height: 50vh;
	}

	.browser-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 10px 12px;
		border-bottom: 1px solid #2c3e50;
		gap: 10px;
		flex-wrap: wrap;
	}

	.breadcrumbs {
		display: flex;
		align-items: center;
		gap: 4px;
		flex-wrap: wrap;
		color: #a0a0a0;
		font-size: 0.85rem;
	}

	.crumb {
		background: transparent;
		border: none;
		color: #4cc9f0;
		cursor: pointer;
		padding: 2px 4px;
		font-size: 0.85rem;
	}

	.crumb.current {
		color: #e0e0e0;
		cursor: default;
	}

	.crumb-sep {
		color: #555;
	}

	.file-list {
		overflow-y: auto;
		flex: 1;
	}

	.file-list-header {
		display: grid;
		grid-template-columns: 1fr 100px;
		padding: 8px 12px;
		background: #1a252f;
		color: #777;
		font-size: 0.75rem;
		text-transform: uppercase;
		letter-spacing: 0.5px;
		position: sticky;
		top: 0;
	}

	.file-row {
		display: grid;
		grid-template-columns: 24px 1fr 100px;
		align-items: center;
		gap: 8px;
		padding: 8px 12px;
		background: transparent;
		border: none;
		border-bottom: 1px solid #1a252f;
		color: #e0e0e0;
		cursor: pointer;
		text-align: left;
		width: 100%;
	}

	.file-row:hover {
		background: #1a252f;
	}

	.file-icon {
		text-align: center;
	}

	.file-name {
		word-break: break-all;
		font-size: 0.9rem;
	}

	.file-size {
		color: #777;
		font-size: 0.85rem;
		text-align: right;
	}

	.search-panel {
		padding: 16px;
		background: #0a0e12;
		border: 1px solid #2c3e50;
		border-radius: 4px;
		max-height: 360px;
		display: flex;
		flex-direction: column;
	}

	.search-panel h3 {
		color: #4cc9f0;
		margin: 0 0 12px;
	}

	.search-controls {
		display: flex;
		gap: 10px;
		margin-bottom: 12px;
		flex-wrap: wrap;
	}

	.search-controls input {
		flex: 1;
		min-width: 200px;
		padding: 8px 12px;
		background: #2c3e50;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
	}

	.hits-header {
		color: #777;
		font-size: 0.85rem;
		margin-bottom: 8px;
	}

	.hits-list {
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.hit-item {
		padding: 10px;
		background: #1a252f;
		border-radius: 4px;
	}

	.hit-app {
		color: #4cc9f0;
		font-weight: 600;
		font-size: 0.9rem;
	}

	.hit-path {
		color: #777;
		font-size: 0.75rem;
		word-break: break-all;
		margin-bottom: 4px;
	}

	.hit-snippet {
		color: #e0e0e0;
		font-size: 0.85rem;
		word-break: break-word;
		white-space: pre-wrap;
	}

	.preview-panel {
		padding: 16px;
		background: #0a0e12;
		border: 1px solid #2c3e50;
		border-radius: 4px;
		display: flex;
		flex-direction: column;
		max-height: 40vh;
	}

	.preview-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 10px;
		margin-bottom: 12px;
		flex-wrap: wrap;
	}

	.preview-header h4 {
		color: #4cc9f0;
		margin: 0;
		word-break: break-all;
	}

	.preview-actions {
		display: flex;
		gap: 8px;
	}

	.preview-content {
		flex: 1;
		overflow: auto;
		background: #0f1419;
		padding: 12px;
		border-radius: 4px;
		color: #e0e0e0;
		font-size: 0.8rem;
		white-space: pre-wrap;
		word-break: break-word;
		margin: 0;
		max-height: 30vh;
	}

	.btn-back {
		display: none;
		padding: 6px 10px;
		background: #2c3e50;
		border: none;
		border-radius: 4px;
		color: #e0e0e0;
		cursor: pointer;
		font-size: 0.85rem;
		margin-bottom: 12px;
	}

	@media (max-width: 640px) {
		.apps-viewer {
			padding: 12px;
			height: auto;
		}

		.toolbar {
			align-items: stretch;
		}

		.controls input,
		.controls select {
			min-width: 0;
			flex: 1;
		}

		.split {
			position: relative;
			flex-direction: column;
		}

		.app-list,
		.app-pane {
			width: 100%;
			box-sizing: border-box;
		}

		.app-list.hidden,
		.app-pane.hidden {
			display: none;
		}

		.app-list {
			max-height: 240px;
		}

		.btn-back {
			display: inline-block;
		}

		.detail-grid {
			grid-template-columns: 1fr;
		}

		.file-list-header,
		.file-row {
			grid-template-columns: 1fr 70px;
		}

		.file-row {
			grid-template-columns: 20px 1fr 70px;
		}

		.search-controls input {
			min-width: 0;
			flex: 1;
		}
	}
</style>
