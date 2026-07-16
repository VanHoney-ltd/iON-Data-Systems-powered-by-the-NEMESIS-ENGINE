<script>
	import { onMount } from 'svelte'
	import {
		GetDeviceStatus,
		PairDevice,
		RefreshDeviceTrust,
		ValidatePairing,
		MountDevice,
		UnmountDevice
	} from '../../wailsjs/go/main/App.js'

	export let onOpenHelp = () => {}

	let status = null
	let loading = false
	let actionLoading = {}
	let error = null
	let pollInterval

	function normalizeStatus(nextStatus) {
		if (!nextStatus) return nextStatus
		return {
			...nextStatus,
			tool_status: Array.isArray(nextStatus.tool_status) ? nextStatus.tool_status : [],
			connected_devices: Array.isArray(nextStatus.connected_devices)
				? nextStatus.connected_devices.map(device => ({
						...device,
						active_mounts: Array.isArray(device?.active_mounts) ? device.active_mounts : []
				  }))
				: []
		}
	}

	async function loadStatus() {
		if (loading) return
		try {
			status = normalizeStatus(await GetDeviceStatus())
			error = null
		} catch (e) {
			error = String(e)
		}
	}

	async function refreshTrust() {
		loading = true
		try {
			status = normalizeStatus(await RefreshDeviceTrust())
			error = null
		} catch (e) {
			error = String(e)
		} finally {
			loading = false
		}
	}

	async function doAction(name, fn) {
		actionLoading = { ...actionLoading, [name]: true }
		try {
			await fn()
			await loadStatus()
		} catch (e) {
			alert(`${name} failed: ${e}`)
		} finally {
			actionLoading = { ...actionLoading, [name]: false }
		}
	}

	function pair(udid) {
		doAction('pair', () => PairDevice(udid))
	}

	function validate(udid) {
		doAction('validate', () => ValidatePairing(udid))
	}

	function mount(udid) {
		doAction('mount', async () => {
			const path = await MountDevice(udid)
			alert(`Mounted at ${path}`)
		})
	}

	function unmount(mountPoint) {
		doAction('unmount', () => UnmountDevice(mountPoint))
	}

	function startPolling() {
		loadStatus()
		pollInterval = setInterval(loadStatus, 5000)
	}

	function stopPolling() {
		if (pollInterval) clearInterval(pollInterval)
	}

	onMount(() => {
		startPolling()
		return stopPolling
	})
</script>

<div class="device-manager">
	<div class="toolbar">
		<h2>Device Manager</h2>
		<div class="controls">
			<button class="btn-secondary" on:click={refreshTrust} disabled={loading}>
				{loading ? 'Refreshing...' : 'Refresh'}
			</button>
			<button class="btn-text" on:click={() => onOpenHelp('devices')}>About devices</button>
		</div>
	</div>

	{#if error}
		<div class="error">{error}</div>
	{/if}

	{#if status}
		<div class="tool-grid">
			{#each status.tool_status as tool}
				<div class="tool-card" class:available={tool.available}>
					<span class="tool-name">{tool.name}</span>
					<span class="tool-state">{tool.available ? 'Available' : 'Missing'}</span>
					{#if tool.detail && tool.available}
						<span class="tool-detail">{tool.detail}</span>
					{/if}
				</div>
			{/each}
		</div>

		{#if status.connected_devices.length === 0}
			<div class="empty">
				<p>No iOS device detected.</p>
				<p class="hint">Connect an iPhone or iPad via USB and unlock it. If this is the first connection, tap "Trust This Computer" on the device.</p>
			</div>
		{:else}
			<div class="device-list">
				{#each status.connected_devices as device}
					<div class="device-card">
						<div class="device-header">
							<div>
								<h3>{device.device_info?.device_name || 'Unknown Device'}</h3>
								<div class="udid">{device.udid}</div>
							</div>
							<span class="state-badge" class:paired={device.pairing_state === 'paired'}>
								{device.pairing_state}
							</span>
						</div>

						{#if device.device_info}
							<div class="info-grid">
								<div class="info-item"><span class="label">Model</span><span class="value">{device.device_info.model_name || device.device_info.product_type || '—'}</span></div>
								<div class="info-item"><span class="label">iOS</span><span class="value">{device.device_info.product_version || '—'}</span></div>
								<div class="info-item"><span class="label">Build</span><span class="value">{device.device_info.build_version || '—'}</span></div>
								<div class="info-item"><span class="label">Serial</span><span class="value">{device.device_info.serial_number || '—'}</span></div>
								<div class="info-item"><span class="label">IMEI</span><span class="value">{device.device_info.imei || '—'}</span></div>
								<div class="info-item"><span class="label">Phone</span><span class="value">{device.device_info.phone_number || '—'}</span></div>
								<div class="info-item"><span class="label">ICCID</span><span class="value">{device.device_info.iccid || '—'}</span></div>
								<div class="info-item"><span class="label">Wi-Fi MAC</span><span class="value">{device.device_info.wifi_address || '—'}</span></div>
								<div class="info-item"><span class="label">Bluetooth</span><span class="value">{device.device_info.bluetooth_address || '—'}</span></div>
							</div>
						{:else if device.info_error}
							<div class="info-error">Could not read device info: {device.info_error}</div>
						{/if}

						{#if device.pairing_detail}
							<div class="pairing-detail">{device.pairing_detail}</div>
						{/if}

						{#if device.trust_refresh_detail}
							<div class="pairing-detail">{device.trust_refresh_detail}</div>
						{/if}

						<div class="actions">
							{#if device.pairing_state !== 'paired'}
								<button class="btn-primary" on:click={() => pair(device.udid)} disabled={actionLoading['pair']}>
									{actionLoading['pair'] ? 'Pairing...' : 'Pair'}
								</button>
							{:else}
								<button class="btn-secondary" on:click={() => validate(device.udid)} disabled={actionLoading['validate']}>
									{actionLoading['validate'] ? 'Validating...' : 'Validate Pairing'}
								</button>
							{/if}

							{#if device.active_mounts.length > 0}
								{#each device.active_mounts as mount}
									<button class="btn-secondary" on:click={() => unmount(mount)} disabled={actionLoading['unmount']}>
										{actionLoading['unmount'] ? 'Unmounting...' : `Unmount ${mount}`}
									</button>
								{/each}
							{:else}
								<button class="btn-secondary" on:click={() => mount(device.udid)} disabled={actionLoading['mount']}>
									{actionLoading['mount'] ? 'Mounting...' : 'Mount Filesystem'}
								</button>
							{/if}
						</div>
					</div>
				{/each}
			</div>
		{/if}
	{:else}
		<div class="loading">Loading device status...</div>
	{/if}
</div>

<style>
	.device-manager {
		padding: 24px;
		height: calc(100vh - 48px);
		overflow-y: auto;
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
		gap: 12px;
		align-items: center;
	}

	.tool-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
		gap: 12px;
		margin-bottom: 24px;
	}

	.tool-card {
		background: #1a252f;
		border: 1px solid #2c3e50;
		border-radius: 6px;
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 4px;
		opacity: 0.6;
	}

	.tool-card.available {
		border-color: #4cc9f0;
		opacity: 1;
	}

	.tool-name {
		font-weight: 600;
		color: #e0e0e0;
	}

	.tool-state {
		font-size: 0.8rem;
		color: #ff9f1c;
	}

	.tool-card.available .tool-state {
		color: #4cc9f0;
	}

	.tool-detail {
		font-size: 0.7rem;
		color: #777;
		word-break: break-all;
	}

	.device-list {
		display: flex;
		flex-direction: column;
		gap: 16px;
	}

	.device-card {
		background: #1a252f;
		border: 1px solid #2c3e50;
		border-radius: 8px;
		padding: 20px;
	}

	.device-header {
		display: flex;
		justify-content: space-between;
		align-items: flex-start;
		margin-bottom: 16px;
		gap: 12px;
	}

	.device-header h3 {
		margin: 0 0 4px;
		color: #e0e0e0;
	}

	.udid {
		font-size: 0.75rem;
		color: #777;
		word-break: break-all;
	}

	.state-badge {
		padding: 6px 12px;
		border-radius: 4px;
		font-size: 0.8rem;
		font-weight: 600;
		text-transform: uppercase;
		background: #3a2a1a;
		color: #ff9f1c;
	}

	.state-badge.paired {
		background: #1a3a2a;
		color: #4cc9f0;
	}

	.info-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
		gap: 12px;
		margin-bottom: 16px;
	}

	@media (max-width: 640px) {
		.device-manager {
			padding: 12px;
			height: auto;
		}

		.device-header {
			flex-direction: column;
			align-items: flex-start;
		}

		.info-grid {
			grid-template-columns: 1fr;
		}

		.tool-grid {
			grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
		}
	}

	.info-item {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.label {
		font-size: 0.7rem;
		color: #777;
		text-transform: uppercase;
		letter-spacing: 0.5px;
	}

	.value {
		color: #e0e0e0;
		font-size: 0.9rem;
		word-break: break-word;
	}

	.pairing-detail, .info-error {
		padding: 10px;
		border-radius: 4px;
		margin-bottom: 16px;
		font-size: 0.85rem;
	}

	.pairing-detail {
		background: #3a2a1a;
		color: #ff9f1c;
	}

	.info-error {
		background: #2a1a1a;
		color: #ff6b6b;
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

	.empty, .loading {
		text-align: center;
		padding: 40px;
		color: #777;
	}

	.empty p:first-child {
		font-size: 1.1rem;
		color: #a0a0a0;
	}

	.hint {
		font-size: 0.85rem;
		max-width: 600px;
		margin: 8px auto 0;
	}

	.error {
		background: #2a1a1a;
		color: #ff6b6b;
		padding: 12px;
		border-radius: 6px;
		margin-bottom: 16px;
	}

	.btn-text {
		background: transparent;
		border: none;
		color: #4cc9f0;
		cursor: pointer;
	}
</style>
