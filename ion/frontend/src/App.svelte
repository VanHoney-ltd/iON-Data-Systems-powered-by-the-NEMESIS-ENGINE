<script>
	import { onMount } from 'svelte'
	import { EventsOn } from '../wailsjs/runtime/runtime.js'
	import {
		ListCases,
		GetAgentCatalog,
		GetCaseSummary,
		CreateCaseFromBackup,
		StartExtraction,
		StopExtraction,
		PullBackup,
		DecryptBackup,
		ListEvidenceTypes,
		GetEvidenceSummary,
		GetEvidenceRecords,
		ExportEvidence
	} from '../wailsjs/go/main/App.js'

	import Sidebar from './components/Sidebar.svelte'
	import TopBar from './components/TopBar.svelte'
	import CaseWizard from './components/CaseWizard.svelte'
	import CaseDashboard from './components/CaseDashboard.svelte'
	import AcquireBackup from './components/AcquireBackup.svelte'
	import HeliosDecrypt from './components/HeliosDecrypt.svelte'
	import AgentCatalog from './components/AgentCatalog.svelte'
	import RunMonitor from './components/RunMonitor.svelte'
	import EvidenceExplorer from './components/EvidenceExplorer.svelte'
	import DeviceManager from './components/DeviceManager.svelte'
	import AppsViewer from './components/AppsViewer.svelte'
	import HelpDrawer from './components/HelpDrawer.svelte'

	// Navigation
	let view = 'dashboard'
	let viewHistory = ['dashboard']
	let historyIndex = 0
	let sidebarCollapsed = false
	let mobileSidebarOpen = false
	let showWizard = false
	let helpOpen = false
	let helpTopic = 'getting-started'

	// Cases
	let cases = []
	let selectedCase = ''
	let caseSummary = null

	// Agents
	let agentCatalog = []

	// Run state
	let isRunning = false
	let currentAgent = ''
	let currentCase = ''
	let backupPassword = ''
	let pulling = false
	let logs = []
	let progress = { step: 0, total: 0, label: '' }
	let acquisitionProgress = { overallPercent: 0, filePercent: 0, label: '', raw: '', active: false }
	let status = 'Ready'

	// Evidence
	let evidenceAgents = []
	let evidenceSummaries = {}
	let evidenceRecords = {}
	let activeEvidenceAgent = ''
	let evidenceLoading = false

	$: canGoBack = historyIndex > 0
	$: canGoForward = historyIndex < viewHistory.length - 1

	function pushView(v) {
		if (view === v) return
		view = v
		showWizard = false
		// Trim forward history and append new view.
		viewHistory = viewHistory.slice(0, historyIndex + 1)
		viewHistory.push(v)
		historyIndex = viewHistory.length - 1
	}

	function goBack() {
		if (historyIndex > 0) {
			historyIndex--
			view = viewHistory[historyIndex]
			showWizard = false
		}
	}

	function goForward() {
		if (historyIndex < viewHistory.length - 1) {
			historyIndex++
			view = viewHistory[historyIndex]
			showWizard = false
		}
	}

	function toggleSidebar() {
		mobileSidebarOpen = !mobileSidebarOpen
		sidebarCollapsed = !sidebarCollapsed
	}

	function closeMobileSidebar() {
		mobileSidebarOpen = false
	}

	function viewEvidence(agent = '') {
		if (agent) {
			activeEvidenceAgent = agent
			if (agent !== 'cerberus') {
				loadEvidence(agent, 1000, '')
			}
		}
		pushView('evidence')
	}

	function selectEvidenceAgent(agent) {
		activeEvidenceAgent = agent
	}

	async function loadCases() {
		try {
			const names = await ListCases()
			cases = names.map(n => ({ id: n, name: n }))
		} catch (e) {
			addLog(`Failed to load cases: ${e}`)
		}
	}

	async function loadCatalog() {
		try {
			agentCatalog = await GetAgentCatalog()
		} catch (e) {
			addLog(`Failed to load agent catalog: ${e}`)
		}
	}

	async function loadCaseSummary(name) {
		if (!name) {
			caseSummary = null
			return
		}
		try {
			caseSummary = await GetCaseSummary(name)
			await loadEvidenceAgents(name)
		} catch (e) {
			caseSummary = null
			addLog(`Failed to load case summary: ${e}`)
		}
	}

	function selectCase(name) {
		selectedCase = name
		loadCaseSummary(name)
		if (view === 'evidence') {
			activeEvidenceAgent = ''
			evidenceRecords = {}
		}
	}

	async function handleCaseCreated(name) {
		showWizard = false
		await loadCases()
		selectCase(name)
		pushView('dashboard')
	}

	function handlePull(caseName) {
		pulling = true
		logs = []
		acquisitionProgress = { overallPercent: 0, filePercent: 0, label: 'Starting backup...', raw: '', active: true }
		status = 'Starting backup acquisition...'
		PullBackup(caseName)
	}

	function handleDecrypt(caseName, backupPath, password, profile) {
		pulling = true
		logs = []
		status = 'Starting HELiOS decryption...'
		DecryptBackup(caseName, backupPath, password, profile)
	}

	async function runAgent(agent) {
		if (!selectedCase) {
			alert('Select a case first')
			return
		}
		currentAgent = agent
		currentCase = selectedCase
		isRunning = true
		logs = []
		progress = { step: 0, total: 0, label: 'Starting...' }
		status = 'Running...'
		pushView('agents')
		try {
			await StartExtraction(agent, selectedCase, backupPassword)
		} catch (e) {
			addLog(`Start error: ${e}`)
			isRunning = false
		}
	}

	function cancelRun() {
		StopExtraction()
		isRunning = false
		status = 'Cancelled'
	}

	async function loadEvidenceAgents(name) {
		try {
			evidenceAgents = await ListEvidenceTypes(name)
		} catch (e) {
			evidenceAgents = []
		}
	}

	async function loadEvidence(agent, limit, recordType = '') {
		if (!selectedCase || !agent) return
		activeEvidenceAgent = agent
		evidenceLoading = true
		try {
			const [summary, records] = await Promise.all([
				GetEvidenceSummary(selectedCase, agent),
				GetEvidenceRecords(selectedCase, agent, recordType, limit)
			])
			evidenceSummaries = { ...evidenceSummaries, [agent]: summary }
			evidenceRecords = { ...evidenceRecords, [agent]: records }
		} catch (e) {
			addLog(`Failed to load evidence: ${e}`)
		} finally {
			evidenceLoading = false
		}
	}

	async function exportEvidence(format) {
		if (!selectedCase || !activeEvidenceAgent) return
		try {
			const path = await ExportEvidence(selectedCase, activeEvidenceAgent, format)
			addLog(`Export ready: ${path}`)
			alert(`Export saved to:\n${path}`)
		} catch (e) {
			addLog(`Export failed: ${e}`)
		}
	}

	function addLog(message) {
		const timestamp = new Date().toLocaleTimeString()
		logs = [...logs, `[${timestamp}] ${message}`]
		if (logs.length > 500) logs = logs.slice(-500)
	}

	function setupEventListeners() {
		const offLog = EventsOn('log', (payload) => {
			addLog(typeof payload === 'string' ? payload : JSON.stringify(payload))
		})

		const offProgress = EventsOn('progress', (payload) => {
			if (typeof payload === 'object' && payload !== null) {
				progress = {
					step: payload.step || 0,
					total: payload.total || 0,
					label: payload.label || ''
				}
			}
		})

		const offAcquisitionProgress = EventsOn('acquisition-progress', (payload) => {
			if (typeof payload === 'object' && payload !== null) {
				acquisitionProgress = {
					overallPercent: Math.max(0, Math.min(100, payload.overallPercent ?? 0)),
					filePercent: Math.max(0, Math.min(100, payload.filePercent ?? 0)),
					label: payload.label || 'Backup running',
					raw: payload.raw || '',
					active: true
				}
			}
		})

		const offStatus = EventsOn('status', (payload) => {
			status = typeof payload === 'string' ? payload : 'Ready'
			if (status === 'Extraction Complete!') {
				isRunning = false
				loadCaseSummary(selectedCase)
			}
			if (typeof status === 'string' && status.startsWith('Error:')) {
				isRunning = false
				pulling = false
				acquisitionProgress = { ...acquisitionProgress, active: false }
			}
			if (status === 'Backup acquisition complete!') {
				pulling = false
				acquisitionProgress = { ...acquisitionProgress, overallPercent: 100, filePercent: 100, label: 'Backup acquisition complete', active: false }
			}
		})

		const offCaseCreated = EventsOn('case-created', async (name) => {
			await loadCases()
			selectCase(name)
		})

		return () => {
			offLog()
			offProgress()
			offAcquisitionProgress()
			offStatus()
			offCaseCreated()
		}
	}

	function openHelp(topic = 'getting-started') {
		helpTopic = topic
		helpOpen = true
	}

	onMount(() => {
		loadCases()
		loadCatalog()
		return setupEventListeners()
	})
</script>

<div class="app" class:sidebar-collapsed={sidebarCollapsed}>
	{#if mobileSidebarOpen}
		<button class="mobile-sidebar-overlay" type="button" aria-label="Close sidebar" on:click={closeMobileSidebar}></button>
	{/if}

	<Sidebar
		view={view}
		setView={pushView}
		selectedCase={selectedCase}
		cases={cases}
		onCaseChange={selectCase}
		onOpenHelp={openHelp}
		collapsed={sidebarCollapsed}
		mobileOpen={mobileSidebarOpen}
		onToggle={toggleSidebar}
		onNav={closeMobileSidebar}
	/>

	<div class="main-area">
		<TopBar
			view={view}
			selectedCase={selectedCase}
			cases={cases}
			onCaseChange={selectCase}
			canGoBack={canGoBack}
			canGoForward={canGoForward}
			onBack={goBack}
			onForward={goForward}
			onToggleSidebar={toggleSidebar}
			onOpenHelp={openHelp}
		/>

		<main class="content">
			{#if showWizard}
				<CaseWizard
					onCreated={handleCaseCreated}
					onCancel={() => showWizard = false}
				/>
			{:else if view === 'dashboard'}
				<CaseDashboard
					selectedCase={selectedCase}
					summary={caseSummary}
					summaries={evidenceSummaries}
					onRunAgent={runAgent}
					onCreateCase={() => showWizard = true}
					onViewEvidence={(agent) => viewEvidence(agent)}
				/>
			{:else if view === 'acquire'}
				<AcquireBackup
					pulling={pulling}
					status={status}
					logs={logs}
					progress={acquisitionProgress}
					onPull={handlePull}
				/>
			{:else if view === 'decrypt'}
				<HeliosDecrypt
					pulling={pulling}
					status={status}
					logs={logs}
					onDecrypt={handleDecrypt}
				/>
			{:else if view === 'agents'}
				<div class="agents-view">
					<AgentCatalog
						agents={agentCatalog}
						selectedCase={selectedCase}
						bind:backupPassword={backupPassword}
						onRun={runAgent}
						onOpenHelp={openHelp}
					/>
					<RunMonitor
						agent={currentAgent}
						caseName={currentCase}
						running={isRunning}
						status={status}
						progress={progress}
						logs={logs}
						onCancel={cancelRun}
						onViewEvidence={() => pushView('evidence')}
					/>
				</div>
			{:else if view === 'devices'}
				<DeviceManager onOpenHelp={openHelp} />
			{:else if view === 'apps'}
				<AppsViewer selectedCase={selectedCase} onOpenHelp={openHelp} />
			{:else if view === 'evidence'}
				<EvidenceExplorer
					selectedCase={selectedCase}
					evidenceAgents={evidenceAgents}
					summaries={evidenceSummaries}
					records={evidenceRecords}
					activeAgent={activeEvidenceAgent}
					onSelectAgent={selectEvidenceAgent}
					onLoadEvidence={loadEvidence}
					onExport={exportEvidence}
					loading={evidenceLoading}
					onOpenHelp={openHelp}
				/>
			{:else if view === 'reports'}
				<div class="placeholder">
					<h2>Reports</h2>
					<p>Go to <strong>Evidence</strong> and use the CSV / JSON / PDF export buttons to generate reports.</p>
					<button class="btn-primary" on:click={() => pushView('evidence')}>Open Evidence</button>
				</div>
			{:else if view === 'help'}
				<div class="placeholder">
					<h2>Help</h2>
					<p>Click the <strong>Need help?</strong> button in the sidebar or the <strong>Help</strong> link on any agent card.</p>
				</div>
			{/if}
		</main>
	</div>

	<HelpDrawer
		open={helpOpen}
		topic={helpTopic}
		onClose={() => helpOpen = false}
		agents={agentCatalog}
	/>
</div>

<style>
	:global(body) {
		margin: 0;
		font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
		background: #0f1419;
		color: #e0e0e0;
	}

	.app {
		display: flex;
		height: 100vh;
		overflow: hidden;
	}

	.main-area {
		flex: 1;
		display: flex;
		flex-direction: column;
		min-width: 0;
	}

	.content {
		flex: 1;
		overflow-y: auto;
		background: #0f1419;
	}

	.mobile-sidebar-overlay {
		display: none;
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.6);
		z-index: 90;
		border: none;
		padding: 0;
		margin: 0;
	}

	@media (max-width: 768px) {
		.mobile-sidebar-overlay {
			display: block;
		}
	}

	.agents-view {
		display: flex;
		flex-direction: column;
		gap: 20px;
		padding-bottom: 40px;
	}

	.placeholder {
		padding: 60px;
		text-align: center;
		color: #a0a0a0;
	}

	.placeholder h2 {
		color: #4cc9f0;
	}

	.btn-primary {
		padding: 12px 24px;
		background: #4cc9f0;
		color: #0f1419;
		border: none;
		border-radius: 4px;
		font-weight: 600;
		cursor: pointer;
		margin-top: 16px;
	}
</style>
