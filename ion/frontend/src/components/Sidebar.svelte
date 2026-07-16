<script>
	export let view
	export let setView
	export let selectedCase
	export let cases = []
	export let onCaseChange
	export let onOpenHelp
	export let collapsed = false
	export let mobileOpen = false
	export let onToggle = () => {}
	export let onNav = () => {}

	function handleNav(id) {
		onNav()
		setView(id)
	}

	const navItems = [
		{ id: 'dashboard', label: 'Case Dashboard', icon: '📁' },
		{ id: 'acquire', label: 'Acquire Backup', icon: '📱' },
		{ id: 'decrypt', label: 'Decrypt Backup', icon: '🔓' },
		{ id: 'agents', label: 'Run Agents', icon: '🔍' },
		{ id: 'devices', label: 'Devices', icon: '🔌' },
		{ id: 'apps', label: 'Apps', icon: '📦' },
		{ id: 'evidence', label: 'Evidence', icon: '📄' },
		{ id: 'reports', label: 'Reports', icon: '📊' },
		{ id: 'help', label: 'Help', icon: '❓' }
	]
</script>

<aside class="sidebar" class:collapsed class:mobile-open={mobileOpen}>
	<div class="brand">
		{#if collapsed}
			<h1>i</h1>
		{:else}
			<h1>iON</h1>
			<span>Forensic Workstation</span>
		{/if}
		<button class="toggle-btn" on:click={onToggle} title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}>
			{collapsed ? '→' : '←'}
		</button>
	</div>

	{#if !collapsed}
		<div class="case-selector">
			<label for="sidebar-case">Active Case</label>
			<select id="sidebar-case" value={selectedCase} on:change={(e) => onCaseChange(e.target.value)}>
				<option value="">-- Select a case --</option>
				{#each cases as c}
					<option value={c.id}>{c.name}</option>
				{/each}
			</select>
			{#if cases.length === 0}
				<p class="hint">Create a case from the dashboard first.</p>
			{/if}
		</div>
	{/if}

	<nav>
		{#each navItems as item}
			<button
				class="nav-item"
				class:active={view === item.id}
				on:click={() => handleNav(item.id)}
				title={item.label}
			>
				<span class="icon">{item.icon}</span>
				{#if !collapsed}
					<span class="label">{item.label}</span>
				{/if}
			</button>
		{/each}
	</nav>

	{#if !collapsed}
		<div class="help-footer">
			<button class="btn-help" on:click={() => onOpenHelp('getting-started')}>
				Need help?
			</button>
		</div>
	{/if}
</aside>

<style>
	.sidebar {
		width: 260px;
		background: #0f1419;
		border-right: 1px solid #2c3e50;
		display: flex;
		flex-direction: column;
		padding: 20px;
		min-height: 100vh;
		box-sizing: border-box;
		transition: width 0.2s, padding 0.2s;
	}

	.sidebar.collapsed {
		width: 72px;
		padding: 16px 8px;
		align-items: center;
	}

	.brand {
		position: relative;
		margin-bottom: 24px;
	}

	.brand h1 {
		color: #4cc9f0;
		margin: 0;
		font-size: 1.8rem;
	}

	.sidebar.collapsed .brand h1 {
		font-size: 1.4rem;
		text-align: center;
	}

	.brand span {
		color: #777;
		font-size: 0.8rem;
		display: block;
		margin-bottom: 8px;
	}

	.toggle-btn {
		position: absolute;
		top: 0;
		right: 0;
		background: #2c3e50;
		border: none;
		border-radius: 4px;
		color: #e0e0e0;
		cursor: pointer;
		padding: 4px 8px;
		font-size: 0.85rem;
	}

	.sidebar.collapsed .toggle-btn {
		position: static;
		margin: 8px auto 0;
	}

	.case-selector {
		margin-bottom: 24px;
	}

	.case-selector label {
		color: #a0a0a0;
		font-size: 0.8rem;
		text-transform: uppercase;
		letter-spacing: 0.5px;
		margin-bottom: 8px;
		display: block;
	}

	.case-selector select {
		width: 100%;
		padding: 10px;
		background: #1a252f;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
	}

	.hint {
		color: #777;
		font-size: 0.75rem;
		margin-top: 8px;
	}

	nav {
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.nav-item {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 12px;
		background: transparent;
		border: none;
		border-radius: 6px;
		color: #a0a0a0;
		font-size: 0.95rem;
		cursor: pointer;
		text-align: left;
		transition: background 0.2s, color 0.2s;
	}

	.sidebar.collapsed .nav-item {
		justify-content: center;
		padding: 12px;
	}

	.nav-item:hover {
		background: #1a252f;
		color: #fff;
	}

	.nav-item.active {
		background: #1a252f;
		color: #4cc9f0;
		border-left: 3px solid #4cc9f0;
	}

	.sidebar.collapsed .nav-item.active {
		border-left: none;
		border-bottom: 3px solid #4cc9f0;
	}

	.icon {
		font-size: 1.1rem;
	}

	.help-footer {
		margin-top: auto;
		padding-top: 20px;
		border-top: 1px solid #2c3e50;
	}

	.btn-help {
		width: 100%;
		padding: 10px;
		background: #2c3e50;
		border: none;
		border-radius: 4px;
		color: #e0e0e0;
		cursor: pointer;
		font-size: 0.9rem;
	}

	.btn-help:hover {
		background: #3d4a55;
	}

	@media (max-width: 768px) {
		.sidebar {
			position: fixed;
			top: 0;
			left: 0;
			width: 260px;
			max-width: 80vw;
			height: 100vh;
			z-index: 100;
			transform: translateX(-100%);
			transition: transform 0.2s ease;
		}

		.sidebar.mobile-open {
			transform: translateX(0);
		}

		.sidebar.collapsed {
			width: 260px;
			padding: 20px;
			align-items: stretch;
		}

		.sidebar.collapsed .brand h1 {
			font-size: 1.8rem;
			text-align: left;
		}

		.sidebar.collapsed .toggle-btn {
			position: absolute;
			top: 0;
			right: 0;
			margin: 0;
		}

		.sidebar.collapsed .nav-item {
			justify-content: flex-start;
			padding: 12px;
		}

		.sidebar.collapsed .nav-item .label {
			display: inline;
		}

		.sidebar.collapsed .case-selector,
		.sidebar.collapsed .help-footer {
			display: block;
		}
	}
</style>
