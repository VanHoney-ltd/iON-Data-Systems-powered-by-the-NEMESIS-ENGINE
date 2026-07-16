<script>
	export let records = []
	export let onOpenHelp = () => {}

	let search = ''
	let selectedContact = null
	let showList = true

	$: contacts = records.filter(r => r.record_type === 'contact')
		.sort((a, b) => String(a.name || '').localeCompare(String(b.name || '')))

	$: filtered = search
		? contacts.filter(c => {
				const term = search.toLowerCase()
				const fields = [
					c.name,
					...(c.phones || []),
					...(c.emails || []),
					...(c.urls || []),
					c.organization,
					c.job_title,
					c.nickname,
					c.note
				].map(v => String(v || '').toLowerCase())
				return fields.some(f => f.includes(term))
			})
		: contacts

	function selectContact(contact) {
		selectedContact = contact
		showList = false
	}

	function backToList() {
		showList = true
		selectedContact = null
	}

	function formatList(values) {
		if (!values || values.length === 0) return '—'
		return values.join(', ')
	}

	function labeledValues(contact) {
		return contact.labeled_values || []
	}
</script>

<div class="contacts-viewer">
	<div class="toolbar">
		<h3>Contacts</h3>
		<div class="controls">
			<input type="text" bind:value={search} placeholder="Search name, phone, email..." />
			<button class="btn-text" on:click={() => onOpenHelp('cerberus')}>About Cerberus</button>
		</div>
	</div>

	<div class="split" class:list-only={showList} class:detail-only={!showList}>
		<aside class="contact-list" class:hidden={!showList}>
			<div class="list-header">{filtered.length} contact{filtered.length === 1 ? '' : 's'}</div>
			{#if filtered.length === 0}
				<div class="empty">No contacts found.</div>
			{:else}
				{#each filtered as contact}
					<button
						class="contact-item"
						class:active={selectedContact === contact}
						on:click={() => selectContact(contact)}
					>
						<div class="contact-name">{contact.name || 'Unknown'}</div>
						<div class="contact-preview">{formatList(contact.phones)}</div>
					</button>
				{/each}
			{/if}
		</aside>

		<main class="contact-pane" class:hidden={showList}>
			{#if selectedContact}
				<div class="contact-card">
					<div class="card-header">
						<button class="btn-back" on:click={backToList}>← Back to contacts</button>
						<h2>{selectedContact.name || 'Unknown'}</h2>
						{#if selectedContact.blocked}
							<span class="badge blocked">Blocked</span>
						{/if}
					</div>

					<div class="fields">
						<div class="field">
							<span class="label">Phone numbers</span>
							<span class="value">{formatList(selectedContact.phones)}</span>
						</div>
						<div class="field">
							<span class="label">Email addresses</span>
							<span class="value">{formatList(selectedContact.emails)}</span>
						</div>
						<div class="field">
							<span class="label">URLs</span>
							<span class="value">{formatList(selectedContact.urls)}</span>
						</div>
						<div class="field">
							<span class="label">Organization</span>
							<span class="value">{selectedContact.organization || '—'}</span>
						</div>
						<div class="field">
							<span class="label">Job title</span>
							<span class="value">{selectedContact.job_title || '—'}</span>
						</div>
						<div class="field">
							<span class="label">Department</span>
							<span class="value">{selectedContact.department || '—'}</span>
						</div>
						<div class="field">
							<span class="label">Nickname</span>
							<span class="value">{selectedContact.nickname || '—'}</span>
						</div>
						<div class="field">
							<span class="label">Birthday</span>
							<span class="value">{selectedContact.birthday || '—'}</span>
						</div>
						<div class="field">
							<span class="label">Note</span>
							<span class="value note">{selectedContact.note || '—'}</span>
						</div>
					</div>

					{#if labeledValues(selectedContact).length > 0}
						<div class="labeled-values">
							<h4>Labeled values</h4>
							<ul>
								{#each labeledValues(selectedContact) as lv}
									<li>
										<span class="lv-type">{lv.value_type}</span>
										<span class="lv-value">{lv.value}</span>
										{#if lv.label}
											<span class="lv-label">({lv.label})</span>
										{/if}
									</li>
								{/each}
							</ul>
						</div>
					{/if}
				</div>
			{:else}
				<div class="empty">Select a contact to view details.</div>
			{/if}
		</main>
	</div>
</div>

<style>
	.contacts-viewer {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
	}

	.toolbar {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 12px;
		gap: 12px;
		flex-wrap: wrap;
	}

	.toolbar h3 {
		color: #4cc9f0;
		margin: 0;
	}

	.controls {
		display: flex;
		gap: 10px;
		align-items: center;
	}

	.controls input {
		padding: 8px 12px;
		background: #2c3e50;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		color: #e0e0e0;
		min-width: 240px;
	}

	.split {
		display: flex;
		flex: 1;
		gap: 16px;
		min-height: 0;
	}

	.contact-list {
		width: 280px;
		overflow-y: auto;
		background: #0f1419;
		border: 1px solid #2c3e50;
		border-radius: 4px;
		padding: 8px;
	}

	.list-header {
		padding: 8px;
		color: #777;
		font-size: 0.8rem;
		border-bottom: 1px solid #2c3e50;
		margin-bottom: 4px;
	}

	.contact-item {
		width: 100%;
		text-align: left;
		padding: 12px;
		background: transparent;
		border: none;
		border-bottom: 1px solid #2c3e50;
		color: #e0e0e0;
		cursor: pointer;
	}

	.contact-item:hover,
	.contact-item.active {
		background: #1a252f;
	}

	.contact-name {
		font-weight: 600;
		margin-bottom: 4px;
		word-break: break-word;
	}

	.contact-preview {
		font-size: 0.75rem;
		color: #777;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.contact-pane {
		flex: 1;
		overflow-y: auto;
		background: #0f1419;
		border: 1px solid #2c3e50;
		border-radius: 4px;
		padding: 20px;
	}

	.contact-card {
		max-width: 700px;
	}

	.card-header {
		display: flex;
		align-items: center;
		gap: 12px;
		margin-bottom: 20px;
		padding-bottom: 12px;
		border-bottom: 1px solid #2c3e50;
	}

	.card-header h2 {
		margin: 0;
		color: #4cc9f0;
	}

	.badge {
		padding: 4px 8px;
		border-radius: 4px;
		font-size: 0.75rem;
		font-weight: 600;
		text-transform: uppercase;
	}

	.badge.blocked {
		background: #ff9f1c;
		color: #0f1419;
	}

	.fields {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
		gap: 16px;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.field .label {
		font-size: 0.75rem;
		color: #777;
		text-transform: uppercase;
		letter-spacing: 0.5px;
	}

	.field .value {
		color: #e0e0e0;
		font-size: 0.95rem;
		word-break: break-word;
	}

	.field .value.note {
		white-space: pre-wrap;
	}

	.labeled-values {
		margin-top: 24px;
		padding-top: 16px;
		border-top: 1px solid #2c3e50;
	}

	.labeled-values h4 {
		color: #4cc9f0;
		margin: 0 0 12px;
	}

	.labeled-values ul {
		list-style: none;
		padding: 0;
		margin: 0;
	}

	.labeled-values li {
		padding: 8px 0;
		border-bottom: 1px solid #1a252f;
		display: flex;
		gap: 10px;
		flex-wrap: wrap;
	}

	.lv-type {
		color: #4cc9f0;
		font-weight: 600;
		text-transform: capitalize;
		min-width: 70px;
	}

	.lv-value {
		color: #e0e0e0;
		word-break: break-word;
	}

	.lv-label {
		color: #777;
		font-size: 0.85rem;
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
		margin-bottom: 8px;
	}

	@media (max-width: 640px) {
		.toolbar {
			align-items: stretch;
		}

		.controls input {
			min-width: 0;
			flex: 1;
		}

		.split {
			position: relative;
		}

		.contact-list,
		.contact-pane {
			width: 100%;
		}

		.contact-list.hidden,
		.contact-pane.hidden {
			display: none;
		}

		.btn-back {
			display: inline-block;
		}

		.card-header {
			flex-direction: column;
			align-items: flex-start;
			gap: 8px;
		}

		.fields {
			grid-template-columns: 1fr;
		}
	}
</style>
