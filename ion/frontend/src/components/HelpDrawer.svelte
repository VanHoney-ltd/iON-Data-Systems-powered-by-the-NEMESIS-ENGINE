<script>
	export let open
	export let topic
	export let onClose
	export let agents = []

	const help = {
		'getting-started': {
			title: 'Getting Started',
			body: `
<p>iON parses iOS backups and extracts evidence. You do not need to write SQL or use the terminal.</p>
<ol>
  <li><strong>Create a case</strong> from the dashboard and point it at your iOS backup folder.</li>
  <li><strong>Run an agent</strong> from the Run Agents page. Start with fast agents like <em>Vigil</em> or <em>Nyx</em>.</li>
  <li><strong>Browse evidence</strong> on the Evidence page and export to CSV/JSON/PDF.</li>
</ol>
<p>Backups are symlinked, not copied, so your original files stay untouched.</p>
			`
		},
		'encrypted': {
			title: 'Encrypted Backups',
			body: `
<p>If your backup is encrypted, the Rust core will ask for the backup password in the terminal where you ran <code>wails dev</code>.</p>
<p>Enter the password there when <code>idevicebackup2</code> asks for it. The app does not collect or pass this acquisition password.</p>
			`
		},
		'apps': {
			title: 'Installed Apps',
			body: `
<p>The Apps page lists every app found in the case backup.</p>
<ul>
  <li><strong>Bundle ID</strong> is the unique app identifier (e.g., <code>com.apple.mobilemail</code>).</li>
  <li><strong>Data size</strong> is the total size of the app's container files.</li>
  <li><strong>Open Container</strong> launches the app's backup folder in your file manager.</li>
</ul>
<p>Apps are discovered from <code>Manifest.db</code> when available, or by scanning <code>AppDomain-*</code> directories in a decrypted backup.</p>
`
		},
		'devices': {
			title: 'Connected Devices',
			body: `
<p>Connect a real iPhone or iPad via USB to see it here.</p>
<ul>
  <li><strong>Pair</strong> establishes trust between this computer and the device.</li>
  <li><strong>Mount Filesystem</strong> uses <code>ifuse</code> to expose the device filesystem.</li>
  <li><strong>Acquire Backup</strong> pulls a new backup from the device.</li>
</ul>
<p>This is not an emulator — it talks to a physical device through libimobiledevice.</p>
`
		},
		'agents': {
			title: 'About Agents',
			body: `
<p>Each agent is a specialized forensic extractor. They run SQLite queries and file parsing for you.</p>
<ul>
  <li><strong>Vigil</strong> — quick system overview.</li>
  <li><strong>Cerberus</strong> — messages, calls, contacts.</li>
  <li><strong>Charon</strong> — photos and videos.</li>
  <li><strong>Echo</strong> — voicemail and audio.</li>
  <li><strong>Hermes</strong> — media catalog and transcription (slow).</li>
  <li><strong>Nyx</strong> — Safari history and bookmarks.</li>
  <li><strong>Plutus</strong> — Apple Wallet data.</li>
  <li><strong>Atlas</strong> — location data.</li>
</ul>
			`
		}
	}

	$: agentHelp = agents.find(a => a.id === topic)
	$: content = agentHelp ? { title: agentHelp.name, body: `<p>${agentHelp.description}</p><p><strong>Scope:</strong> ${agentHelp.scope}</p><p><strong>Estimated time:</strong> ${agentHelp.estimatedTime}</p><p><strong>Tip:</strong> ${agentHelp.tip}</p>` } : (help[topic] || help['getting-started'])
</script>

{#if open}
	<button class="overlay" type="button" aria-label="Close help drawer" on:click={onClose}></button>
	<aside class="drawer">
		<div class="drawer-header">
			<h2>{content.title}</h2>
			<button class="close" on:click={onClose}>×</button>
		</div>
		<div class="drawer-body">
			{@html content.body}
		</div>
	</aside>
{/if}

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.6);
		z-index: 100;
	}

	.drawer {
		position: fixed;
		top: 0;
		right: 0;
		width: 420px;
		max-width: 90vw;
		height: 100vh;
		background: #1a252f;
		border-left: 1px solid #2c3e50;
		z-index: 101;
		padding: 24px;
		box-sizing: border-box;
		overflow-y: auto;
	}

	.drawer-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 20px;
		border-bottom: 1px solid #2c3e50;
		padding-bottom: 12px;
	}

	.drawer-header h2 {
		color: #4cc9f0;
		margin: 0;
	}

	.close {
		background: transparent;
		border: none;
		color: #a0a0a0;
		font-size: 1.8rem;
		cursor: pointer;
	}

	.drawer-body {
		color: #a0a0a0;
		line-height: 1.6;
	}

	.drawer-body :global(p) {
		margin: 0 0 12px;
	}

	.drawer-body :global(ul), .drawer-body :global(ol) {
		margin: 0 0 16px 20px;
	}

	.drawer-body :global(li) {
		margin-bottom: 6px;
	}

	.drawer-body :global(code) {
		background: #2c3e50;
		padding: 2px 6px;
		border-radius: 3px;
		color: #4cc9f0;
	}
</style>
