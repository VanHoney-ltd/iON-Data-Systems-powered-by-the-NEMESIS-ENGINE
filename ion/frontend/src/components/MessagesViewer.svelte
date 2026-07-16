<script>
	import { onDestroy, onMount, tick } from 'svelte'
	import { BatchMessageAction, OpenAttachment } from '../../wailsjs/go/main/App.js'

	export let selectedCase = ''
	export let agent = 'cerberus'
	export let onOpenHelp = () => {}

	const CONTACT_PAGE_SIZE = 200
	const THREAD_PAGE_SIZE = 200
	const MESSAGE_PAGE_SIZE = 250
	const CONTACT_ROW_HEIGHT = 58
	const THREAD_ROW_HEIGHT = 76
	const MESSAGE_ROW_HEIGHT = 170
	const OVERSCAN = 5

	let mounted = false
	let activeCase = ''
	let indexStatus = null
	let indexStatusError = ''

	let contactSearchInput = ''
	let contactSearch = ''
	let contacts = []
	let contactsTotal = 0
	let contactsHasMore = false
	let contactsLoading = false
	let contactsError = ''
	let selectedHandles = new Set()

	let messageSearchInput = ''
	let messageSearch = ''
	let threads = []
	let threadsTotal = 0
	let threadsHasMore = false
	let threadsLoading = false
	let threadsError = ''
	let selectedThreadId = ''

	let messages = []
	let messagesTotal = 0
	let messagesHasMore = false
	let messagesLoading = false
	let messagesError = ''

	let selectedIds = new Set()
	let lastResult = null
	let exportStatus = ''
	let exporting = false

	let contactScroller
	let threadScroller
	let messageScroller
	let contactScrollTop = 0
	let threadScrollTop = 0
	let messageScrollTop = 0
	let contactViewportHeight = 240
	let threadViewportHeight = 320
	let messageViewportHeight = 500

	let caseToken = 0
	let contactsToken = 0
	let threadsToken = 0
	let messagesToken = 0
	let contactSearchTimer
	let messageSearchTimer
	let indexPollTimer

	$: selectedCount = selectedIds.size

	$: contactStart = Math.max(0, Math.floor(contactScrollTop / CONTACT_ROW_HEIGHT) - OVERSCAN)
	$: contactEnd = Math.min(
		contacts.length,
		Math.ceil((contactScrollTop + contactViewportHeight) / CONTACT_ROW_HEIGHT) + OVERSCAN
	)
	$: visibleContacts = contacts.slice(contactStart, contactEnd)

	$: threadStart = Math.max(0, Math.floor(threadScrollTop / THREAD_ROW_HEIGHT) - OVERSCAN)
	$: threadEnd = Math.min(
		threads.length,
		Math.ceil((threadScrollTop + threadViewportHeight) / THREAD_ROW_HEIGHT) + OVERSCAN
	)
	$: visibleThreads = threads.slice(threadStart, threadEnd)

	$: messageStart = Math.max(0, Math.floor(messageScrollTop / MESSAGE_ROW_HEIGHT) - OVERSCAN)
	$: messageEnd = Math.min(
		messages.length,
		Math.ceil((messageScrollTop + messageViewportHeight) / MESSAGE_ROW_HEIGHT) + OVERSCAN
	)
	$: visibleMessages = messages.slice(messageStart, messageEnd)

	$: if (mounted && selectedCase !== activeCase) {
		activateCase(selectedCase)
	}

	onMount(() => {
		mounted = true
		updateViewportHeights()
		activateCase(selectedCase)
	})

	onDestroy(() => {
		mounted = false
		clearTimeout(contactSearchTimer)
		clearTimeout(messageSearchTimer)
		clearTimeout(indexPollTimer)
	})

	function appMethod(name) {
		const method = window?.go?.main?.App?.[name]
		if (typeof method !== 'function') {
			throw new Error(`${name} is not available. Rebuild the Wails application.`)
		}
		return method
	}

	function callApp(name, ...args) {
		return appMethod(name)(...args)
	}

	function errorMessage(error) {
		if (error instanceof Error) return error.message
		return String(error || 'Unknown error')
	}

	function pageItems(result, keys) {
		if (Array.isArray(result)) return result
		for (const key of keys) {
			if (Array.isArray(result?.[key])) return result[key]
		}
		return []
	}

	function pageTotal(result, fallback) {
		const value = result?.total ?? result?.Total ?? result?.totalCount ?? result?.total_count
		const number = Number(value)
		return Number.isFinite(number) ? number : fallback
	}

	function pageHasMore(result, loaded, total) {
		const value = result?.hasMore ?? result?.HasMore ?? result?.has_more
		if (typeof value === 'boolean') return value
		return loaded < total
	}

	function contactHandle(contact) {
		return String(contact?.handle ?? contact?.Handle ?? contact?.phoneNumber ?? contact?.phone_number ?? '')
	}

	function contactName(contact) {
		const name = contact?.displayName ?? contact?.DisplayName ?? contact?.display_name ?? contact?.name
		return String(name || contactHandle(contact) || 'Unknown contact')
	}

	function contactCount(contact) {
		return Number(contact?.messageCount ?? contact?.MessageCount ?? contact?.message_count ?? contact?.count ?? 0)
	}

	function threadId(thread) {
		return String(thread?.threadId ?? thread?.ThreadID ?? thread?.thread_id ?? thread?.id ?? '')
	}

	function threadName(thread) {
		const names = thread?.displayName ?? thread?.DisplayName ?? thread?.display_name ?? thread?.name
		if (names) return String(names)
		const handles = thread?.participantNames ?? thread?.ParticipantNames ?? thread?.participant_names ??
			thread?.handles ?? thread?.Handles ?? thread?.participants
		if (Array.isArray(handles) && handles.length > 0) return handles.join(', ')
		return threadId(thread) || 'Unknown conversation'
	}

	function threadCount(thread) {
		return Number(thread?.messageCount ?? thread?.MessageCount ?? thread?.message_count ?? thread?.count ?? 0)
	}

	function threadLatest(thread) {
		return thread?.lastTimestampUtc ?? thread?.LastTimestampUtc ?? thread?.last_timestamp_utc ??
			thread?.latestTimestamp ?? thread?.LatestTimestamp ?? thread?.latest_timestamp ?? thread?.latest ?? ''
	}

	function threadPreview(thread) {
		return String(thread?.lastMessage ?? thread?.LastMessage ?? thread?.last_message ?? thread?.preview ?? '')
	}

	function messageId(message, index = 0) {
		return String(
			message?._id ?? message?.id ?? message?.ID ?? message?.messageId ?? message?.message_id ?? message?.guid ?? `message-${index}`
		)
	}

	function messageText(message) {
		return String(message?.text ?? message?.Text ?? message?.body ?? '')
	}

	function messageTimestamp(message) {
		return message?.timestampUtc ?? message?.TimestampUtc ?? message?.timestamp_utc ??
			message?.timestamp ?? message?.Timestamp ?? message?.date ?? message?.sentAt ?? message?.sent_at ?? ''
	}

	function messageService(message) {
		return String(message?.service ?? message?.Service ?? '')
	}

	function messageSender(message) {
		return String(
			message?.displayName ?? message?.display_name ?? message?.senderName ?? message?.sender_name ?? message?.handle ?? message?.phone_number ?? ''
		)
	}

	function formatTime(timestamp) {
		if (!timestamp) return ''
		const date = new Date(timestamp)
		if (Number.isNaN(date.getTime())) return String(timestamp)
		return date.toLocaleString()
	}

	function isSent(message) {
		if (message?.isSent === true || message?.is_sent === true) return true
		const direction = String(message?.direction ?? message?.Direction ?? '').toLowerCase()
		return direction === 'sent' || direction === 'outgoing' || direction === 'outbound'
	}

	function getAttachments(message) {
		const embedded = message?.attachments ?? message?.Attachments
		if (Array.isArray(embedded)) {
			return embedded
				.map(attachment => {
					if (typeof attachment === 'string') return { path: attachment, mime: 'unknown' }
					return {
						path: attachment?.exportPath ?? attachment?.export_path ?? attachment?.path ?? '',
						mime: attachment?.mimeType ?? attachment?.mime_type ?? attachment?.mime ?? 'unknown'
					}
				})
				.filter(attachment => attachment.path)
		}

		const paths = message?.attachmentExportPaths ?? message?.attachment_export_paths ?? []
		const mimes = message?.attachmentMimeTypes ?? message?.attachment_mime_types ?? []
		return (Array.isArray(paths) ? paths : [])
			.map((path, index) => ({ path, mime: mimes[index] || 'unknown' }))
			.filter(attachment => attachment.path)
	}

	function attachmentName(path) {
		const parts = String(path || '').split(/[\\/]/)
		return parts[parts.length - 1] || 'Attachment'
	}

	function indexDescription(status) {
		if (!status) return 'Preparing message index...'
		const state = String(status.state ?? status.State ?? status.status ?? status.Status ?? '').toLowerCase()
		if (state === 'error') {
			const detail = status.error ?? status.Error
			return detail ? `Message index failed: ${detail}` : 'Message index failed'
		}
		const text = status.message ?? status.Message
		if (text) return String(text)
		if (state === 'ready' || status.ready === true || status.Ready === true) return 'Message index ready'
		return 'Preparing message index...'
	}

	function indexReady(status) {
		const ready = status?.ready ?? status?.Ready
		if (typeof ready === 'boolean') return ready
		return String(status?.state ?? status?.State ?? status?.status ?? status?.Status ?? '').toLowerCase() === 'ready'
	}

	function indexWorking(status) {
		const working = status?.building ?? status?.Building ?? status?.indexing ?? status?.Indexing
		if (typeof working === 'boolean') return working
		const value = String(status?.state ?? status?.State ?? status?.status ?? status?.Status ?? '').toLowerCase()
		return value === 'not_indexed' || value === 'building' || value === 'indexing' || value === 'loading'
	}

	function indexFailed(status) {
		return String(status?.state ?? status?.State ?? '').toLowerCase() === 'error'
	}

	function currentHandles() {
		return Array.from(selectedHandles).sort()
	}

	async function activateCase(caseName) {
		activeCase = caseName
		const token = ++caseToken
		contactsToken++
		threadsToken++
		messagesToken++
		clearTimeout(indexPollTimer)

		indexStatus = null
		indexStatusError = ''
		contactSearchInput = ''
		contactSearch = ''
		messageSearchInput = ''
		messageSearch = ''
		contacts = []
		contactsTotal = 0
		contactsHasMore = false
		threads = []
		threadsTotal = 0
		threadsHasMore = false
		messages = []
		messagesTotal = 0
		messagesHasMore = false
		contactsLoading = false
		threadsLoading = false
		messagesLoading = false
		selectedHandles = new Set()
		selectedThreadId = ''
		selectedIds = new Set()
		contactsError = ''
		threadsError = ''
		messagesError = ''
		lastResult = null
		exportStatus = ''

		if (!caseName) return

		refreshIndexStatus(caseName, token)
		await loadContacts(true)
		if (token === caseToken && caseName === activeCase) await loadThreads(true)
	}

	async function refreshIndexStatus(caseName = activeCase, token = caseToken) {
		if (!caseName) return
		try {
			const status = await callApp('GetMessageIndexStatus', caseName)
			if (!mounted || token !== caseToken || caseName !== activeCase) return
			indexStatus = status
			indexStatusError = ''
			if (!indexReady(status) && indexWorking(status)) {
				indexPollTimer = setTimeout(() => refreshIndexStatus(caseName, token), 1200)
			}
		} catch (error) {
			if (token !== caseToken) return
			indexStatusError = errorMessage(error)
		}
	}

	async function loadContacts(reset = false) {
		if (!activeCase || (!reset && contactsLoading)) return
		if (!reset && !contactsHasMore) return

		const token = reset ? ++contactsToken : contactsToken
		const offset = reset ? 0 : contacts.length
		if (reset) {
			contacts = []
			contactsTotal = 0
			contactsHasMore = false
			contactScrollTop = 0
		}
		contactsLoading = true
		if (reset) contactsError = ''

		try {
			const result = await callApp(
				'GetMessageContacts',
				activeCase,
				contactSearch,
				CONTACT_PAGE_SIZE,
				offset
			)
			if (token !== contactsToken) return

			const items = pageItems(result, ['contacts', 'Contacts', 'items', 'Items'])
			contacts = reset ? items : [...contacts, ...items]
			contactsTotal = pageTotal(result, contacts.length)
			contactsHasMore = pageHasMore(result, contacts.length, contactsTotal)
			contactsError = ''
			refreshIndexStatus(activeCase, caseToken)
			if (reset) {
				contactScrollTop = 0
				await tick()
				if (contactScroller) contactScroller.scrollTop = 0
			}
		} catch (error) {
			if (token === contactsToken) contactsError = errorMessage(error)
		} finally {
			if (token === contactsToken) contactsLoading = false
		}
	}

	async function loadThreads(reset = false) {
		if (!activeCase || (!reset && threadsLoading)) return
		if (!reset && !threadsHasMore) return

		const token = reset ? ++threadsToken : threadsToken
		const offset = reset ? 0 : threads.length
		const handles = currentHandles()
		const searchTerm = messageSearch
		if (reset) {
			threads = []
			threadsTotal = 0
			threadsHasMore = false
			threadScrollTop = 0
			selectedThreadId = ''
			selectedIds = new Set()
			clearMessages()
		}
		threadsLoading = true
		if (reset) threadsError = ''

		try {
			const result = await callApp(
				'GetMessageThreads',
				activeCase,
				handles,
				searchTerm,
				THREAD_PAGE_SIZE,
				offset
			)
			if (token !== threadsToken) return

			const items = pageItems(result, ['threads', 'Threads', 'items', 'Items'])
			threads = reset ? items : [...threads, ...items]
			threadsTotal = pageTotal(result, threads.length)
			threadsHasMore = pageHasMore(result, threads.length, threadsTotal)
			threadsError = ''

			if (reset) {
				threadScrollTop = 0
				await tick()
				if (threadScroller) threadScroller.scrollTop = 0
				const firstThread = items[0]
				selectedThreadId = firstThread ? threadId(firstThread) : ''
				selectedIds = new Set()
				await loadMessages(true)
			}
		} catch (error) {
			if (token === threadsToken) {
				threadsError = errorMessage(error)
				if (reset) {
					selectedThreadId = ''
					clearMessages()
				}
			}
		} finally {
			if (token === threadsToken) threadsLoading = false
		}
	}

	function messageQuery(offset, thread = selectedThreadId) {
		return {
			handles: currentHandles(),
			threadId: thread,
			search: messageSearch,
			limit: MESSAGE_PAGE_SIZE,
			offset
		}
	}

	async function loadMessages(reset = false) {
		if (!activeCase || !selectedThreadId || (!reset && messagesLoading)) return
		if (!reset && !messagesHasMore) return

		const token = reset ? ++messagesToken : messagesToken
		const offset = reset ? 0 : messages.length
		const query = messageQuery(offset)
		if (reset) {
			messages = []
			messagesTotal = 0
			messagesHasMore = false
			messageScrollTop = 0
		}
		messagesLoading = true
		if (reset) messagesError = ''

		try {
			const result = await callApp('QueryMessages', activeCase, query)
			if (token !== messagesToken || query.threadId !== selectedThreadId) return

			const items = pageItems(result, ['messages', 'Messages', 'items', 'Items'])
			messages = reset ? items : [...messages, ...items]
			messagesTotal = pageTotal(result, messages.length)
			messagesHasMore = pageHasMore(result, messages.length, messagesTotal)
			messagesError = ''

			if (reset) {
				messageScrollTop = 0
				await tick()
				if (messageScroller) messageScroller.scrollTop = 0
			}
		} catch (error) {
			if (token === messagesToken) messagesError = errorMessage(error)
		} finally {
			if (token === messagesToken) messagesLoading = false
		}

		if (
			token === messagesToken &&
			messagesHasMore &&
			messages.length * MESSAGE_ROW_HEIGHT < messageViewportHeight + MESSAGE_ROW_HEIGHT
		) {
			loadMessages(false)
		}
	}

	function clearMessages() {
		messagesToken++
		messages = []
		messagesTotal = 0
		messagesHasMore = false
		messagesError = ''
		messagesLoading = false
		messageScrollTop = 0
	}

	function scheduleContactSearch() {
		clearTimeout(contactSearchTimer)
		contactSearchTimer = setTimeout(() => {
			contactSearch = contactSearchInput.trim()
			loadContacts(true)
		}, 220)
	}

	function scheduleMessageSearch() {
		clearTimeout(messageSearchTimer)
		messageSearchTimer = setTimeout(() => {
			messageSearch = messageSearchInput.trim()
			loadThreads(true)
		}, 280)
	}

	function toggleHandle(handle) {
		if (!handle) return
		const next = new Set(selectedHandles)
		if (next.has(handle)) next.delete(handle)
		else next.add(handle)
		selectedHandles = next
		selectedThreadId = ''
		clearMessages()
		loadThreads(true)
	}

	function clearContactFilter() {
		if (selectedHandles.size === 0) return
		selectedHandles = new Set()
		selectedThreadId = ''
		clearMessages()
		loadThreads(true)
	}

	function selectThread(id) {
		if (!id || id === selectedThreadId) return
		selectedThreadId = id
		selectedIds = new Set()
		clearMessages()
		loadMessages(true)
	}

	function handleContactScroll(event) {
		const target = event.currentTarget
		contactScrollTop = target.scrollTop
		contactViewportHeight = target.clientHeight
		if (target.scrollTop + target.clientHeight >= target.scrollHeight - CONTACT_ROW_HEIGHT * 4) {
			loadContacts(false)
		}
	}

	function handleThreadScroll(event) {
		const target = event.currentTarget
		threadScrollTop = target.scrollTop
		threadViewportHeight = target.clientHeight
		if (target.scrollTop + target.clientHeight >= target.scrollHeight - THREAD_ROW_HEIGHT * 4) {
			loadThreads(false)
		}
	}

	function handleMessageScroll(event) {
		const target = event.currentTarget
		messageScrollTop = target.scrollTop
		messageViewportHeight = target.clientHeight
		if (target.scrollTop + target.clientHeight >= target.scrollHeight - MESSAGE_ROW_HEIGHT * 4) {
			loadMessages(false)
		}
	}

	function updateViewportHeights() {
		if (contactScroller) contactViewportHeight = contactScroller.clientHeight
		if (threadScroller) threadViewportHeight = threadScroller.clientHeight
		if (messageScroller) messageViewportHeight = messageScroller.clientHeight
	}

	function toggleMessageId(id) {
		const next = new Set(selectedIds)
		if (next.has(id)) next.delete(id)
		else next.add(id)
		selectedIds = next
	}

	function selectLoadedMessages() {
		const next = new Set(selectedIds)
		messages.forEach((message, index) => next.add(messageId(message, index)))
		selectedIds = next
	}

	function clearSelection() {
		selectedIds = new Set()
	}

	async function openAttachment(path) {
		try {
			await OpenAttachment(path)
		} catch (error) {
			alert(`Could not open attachment: ${errorMessage(error)}`)
		}
	}

	async function runAction(action) {
		if (selectedCount === 0) return
		try {
			const result = await BatchMessageAction(
				selectedCase,
				agent || 'cerberus',
				action,
				Array.from(selectedIds)
			)
			lastResult = result
			if (action === 'redact') {
				clearSelection()
				await loadMessages(true)
			}
		} catch (error) {
			lastResult = { message: `Action failed: ${errorMessage(error)}` }
		}
	}

	async function exportMessages(format) {
		if (!selectedCase || exporting) return
		exporting = true
		exportStatus = `Creating ${format.toUpperCase()} export...`
		try {
			const query = {
				handles: currentHandles(),
				threadId: '',
				search: messageSearch,
				limit: 0,
				offset: 0
			}
			const result = await callApp('ExportMessageQuery', selectedCase, query, format)
			const path = typeof result === 'string' ? result : result?.path ?? result?.Path ?? ''
			const count = typeof result === 'object' ? result?.count ?? result?.Count : null
			exportStatus = count !== null && count !== undefined
				? `${Number(count).toLocaleString()} messages exported to ${path}`
				: `Export ready: ${path}`
		} catch (error) {
			exportStatus = `Export failed: ${errorMessage(error)}`
		} finally {
			exporting = false
		}
	}
</script>

<svelte:window on:resize={updateViewportHeights} />

<div class="messages-viewer">
	<div class="viewer-toolbar">
		<div class="title-group">
			<h3>Messages</h3>
			{#if selectedCase}
				<span class="case-name">{selectedCase}</span>
			{/if}
		</div>
		<div class="message-search">
			<label for="message-search">Search message results</label>
			<input
				id="message-search"
				type="search"
				bind:value={messageSearchInput}
				on:input={scheduleMessageSearch}
				placeholder="Text, sender, or service"
			/>
		</div>
		<div class="toolbar-actions">
			<button type="button" class="btn-secondary" disabled={exporting || !selectedCase} on:click={() => exportMessages('json')}>JSON</button>
			<button type="button" class="btn-secondary" disabled={exporting || !selectedCase} on:click={() => exportMessages('csv')}>CSV</button>
			<button type="button" class="btn-link" on:click={() => onOpenHelp('cerberus')}>Cerberus help</button>
		</div>
	</div>

	{#if indexStatus && !indexReady(indexStatus)}
		<div class="status-banner" class:warning={indexFailed(indexStatus)} aria-live="polite">
			<span class:working={indexWorking(indexStatus)}></span>
			{indexDescription(indexStatus)}
		</div>
	{:else if indexStatusError}
		<div class="status-banner warning" aria-live="polite">Index status unavailable: {indexStatusError}</div>
	{/if}

	<div class="filter-summary">
		<span>
			{selectedHandles.size === 0
				? 'All contacts'
				: `${selectedHandles.size.toLocaleString()} contact${selectedHandles.size === 1 ? '' : 's'} selected`}
		</span>
		<span>{threadsTotal.toLocaleString()} conversation results</span>
		{#if selectedHandles.size > 0}
			<button type="button" class="btn-link" on:click={clearContactFilter}>Clear contact filter</button>
		{/if}
		{#if exportStatus}
			<span class="export-status" title={exportStatus}>{exportStatus}</span>
		{/if}
	</div>

	<div class="viewer-layout">
		<aside class="contacts-pane" aria-label="Contact filters">
			<div class="pane-header">
				<div>
					<h4>Contacts</h4>
					<span>{contactsTotal.toLocaleString()} available</span>
				</div>
				<input
					type="search"
					bind:value={contactSearchInput}
					on:input={scheduleContactSearch}
					placeholder="Find contact"
					aria-label="Find contact"
				/>
			</div>

			{#if contactsError}
				<div class="inline-error">{contactsError}</div>
			{/if}
			<div class="virtual-list contacts-list" bind:this={contactScroller} on:scroll={handleContactScroll}>
				<div style={`height: ${contactStart * CONTACT_ROW_HEIGHT}px`}></div>
				{#each visibleContacts as contact, localIndex (contactHandle(contact) || contactStart + localIndex)}
					{@const handle = contactHandle(contact)}
					<label class="contact-row" class:selected={selectedHandles.has(handle)} style={`height: ${CONTACT_ROW_HEIGHT}px`}>
						<input
							type="checkbox"
							checked={selectedHandles.has(handle)}
							disabled={!handle}
							on:change={() => toggleHandle(handle)}
						/>
						<span class="contact-copy">
							<strong title={contactName(contact)}>{contactName(contact)}</strong>
							<span title={handle || 'No handle'}>{handle || 'No handle'} · {contactCount(contact).toLocaleString()} conversation messages</span>
						</span>
					</label>
				{/each}
				<div style={`height: ${Math.max(0, contacts.length - contactEnd) * CONTACT_ROW_HEIGHT}px`}></div>
				{#if contactsLoading}
					<div class="list-loading">Loading contacts...</div>
				{:else if contacts.length === 0 && !contactsError}
					<div class="empty small">No contacts match this search.</div>
				{/if}
			</div>
		</aside>

		<aside class="threads-pane" aria-label="Conversations">
			<div class="pane-header compact">
				<div>
					<h4>Conversations</h4>
					<span>{threadsTotal.toLocaleString()} matching</span>
				</div>
			</div>
			{#if threadsError}
				<div class="inline-error">{threadsError}</div>
			{/if}
			<div class="virtual-list threads-list" bind:this={threadScroller} on:scroll={handleThreadScroll}>
				<div style={`height: ${threadStart * THREAD_ROW_HEIGHT}px`}></div>
				{#each visibleThreads as thread, localIndex (threadId(thread) || threadStart + localIndex)}
					{@const id = threadId(thread)}
					<button
						type="button"
						class="thread-row"
						class:selected={selectedThreadId === id}
						style={`height: ${THREAD_ROW_HEIGHT}px`}
						on:click={() => selectThread(id)}
					>
						<span class="thread-topline">
							<strong title={threadName(thread)}>{threadName(thread)}</strong>
							<span>{threadCount(thread).toLocaleString()}</span>
						</span>
						<span class="thread-preview">{threadPreview(thread) || formatTime(threadLatest(thread))}</span>
						{#if threadPreview(thread)}
							<span class="thread-time">{formatTime(threadLatest(thread))}</span>
						{/if}
					</button>
				{/each}
				<div style={`height: ${Math.max(0, threads.length - threadEnd) * THREAD_ROW_HEIGHT}px`}></div>
				{#if threadsLoading}
					<div class="list-loading">Loading conversations...</div>
				{:else if threads.length === 0 && !threadsError}
					<div class="empty small">No conversations match these filters.</div>
				{/if}
			</div>
		</aside>

		<main class="chat-pane">
			{#if selectedThreadId}
				<div class="chat-header">
					<div>
						<h4>{threadName(threads.find(thread => threadId(thread) === selectedThreadId))}</h4>
						<span>{messagesTotal.toLocaleString()} matching messages · {messages.length.toLocaleString()} loaded</span>
					</div>
					<div class="selection-actions">
						<span>{selectedCount.toLocaleString()} selected</span>
						<button type="button" class="btn-link" disabled={messages.length === 0} on:click={selectLoadedMessages}>Select loaded</button>
						<button type="button" class="btn-link" disabled={selectedCount === 0} on:click={clearSelection}>Clear</button>
					</div>
				</div>

				<div class="batch-bar">
					<button type="button" disabled={selectedCount === 0} on:click={() => runAction('evidence')}>Add to Evidence</button>
					<button type="button" disabled={selectedCount === 0} on:click={() => runAction('review')}>Mark for Review</button>
					<button type="button" class="danger" disabled={selectedCount === 0} on:click={() => runAction('redact')}>Redact</button>
					<button type="button" disabled={selectedCount === 0} on:click={() => runAction('print')}>Print Report</button>
					{#if lastResult}
						<span title={lastResult.path || ''}>{lastResult.message}</span>
					{/if}
				</div>

				{#if messagesError}
					<div class="inline-error">{messagesError}</div>
				{/if}
				<div class="messages-viewport" bind:this={messageScroller} on:scroll={handleMessageScroll}>
					<div style={`height: ${messageStart * MESSAGE_ROW_HEIGHT}px`}></div>
					{#each visibleMessages as message, localIndex (messageId(message, messageStart + localIndex))}
						{@const id = messageId(message, messageStart + localIndex)}
						{@const attachments = getAttachments(message)}
						<div
							class="message-slot"
							class:sent={isSent(message)}
							class:received={!isSent(message)}
							style={`height: ${MESSAGE_ROW_HEIGHT}px`}
						>
							<label class="message-checkbox" title="Select message">
								<input type="checkbox" checked={selectedIds.has(id)} on:change={() => toggleMessageId(id)} />
							</label>
							<div class="bubble">
								<div class="text">{messageText(message) || '(no text)'}</div>
								{#if attachments.length > 0}
									<div class="attachments">
										{#each attachments as attachment}
											<button type="button" on:click={() => openAttachment(attachment.path)} title={attachment.path}>
												<span>{attachmentName(attachment.path)}</span>
												<small>{attachment.mime}</small>
											</button>
										{/each}
									</div>
								{/if}
								<div class="message-meta">
									{#if messageSender(message)}<span>{messageSender(message)}</span>{/if}
									<span>{formatTime(messageTimestamp(message))}</span>
									{#if messageService(message)}<span>{messageService(message)}</span>{/if}
								</div>
							</div>
						</div>
					{/each}
					<div style={`height: ${Math.max(0, messages.length - messageEnd) * MESSAGE_ROW_HEIGHT}px`}></div>
					{#if messagesLoading}
						<div class="list-loading messages-loading">Loading messages...</div>
					{:else if messages.length === 0 && !messagesError}
						<div class="empty">No messages match this search.</div>
					{:else if !messagesHasMore && messages.length > 0}
						<div class="end-marker">End of conversation</div>
					{/if}
				</div>
			{:else}
				<div class="empty">Select contacts with matching conversations. The newest conversation opens automatically.</div>
			{/if}
		</main>
	</div>
</div>

<style>
	.messages-viewer {
		display: flex;
		flex: 1;
		flex-direction: column;
		min-height: 0;
		color: #e4e9ed;
	}

	.viewer-toolbar,
	.filter-summary,
	.pane-header,
	.chat-header,
	.batch-bar,
	.thread-topline,
	.selection-actions,
	.toolbar-actions {
		display: flex;
		align-items: center;
	}

	.viewer-toolbar {
		justify-content: space-between;
		gap: 14px;
		padding: 0 0 12px;
	}

	.title-group {
		display: flex;
		align-items: baseline;
		gap: 9px;
		min-width: 0;
	}

	h3,
	h4 {
		margin: 0;
		letter-spacing: 0;
	}

	h3 {
		color: #4cc9f0;
		font-size: 1.05rem;
	}

	h4 {
		font-size: 0.9rem;
	}

	.case-name,
	.pane-header span,
	.chat-header span,
	.filter-summary,
	.message-meta,
	.thread-time,
	.thread-preview,
	.contact-copy span {
		color: #8e9aa4;
		font-size: 0.74rem;
	}

	.message-search {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 300px;
		max-width: 520px;
		flex: 1;
	}

	.message-search label {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}

	input[type='search'] {
		box-sizing: border-box;
		width: 100%;
		height: 34px;
		padding: 6px 10px;
		border: 1px solid #3d4a55;
		border-radius: 4px;
		outline: none;
		background: #111920;
		color: #e4e9ed;
	}

	input[type='search']:focus {
		border-color: #4cc9f0;
		box-shadow: 0 0 0 1px #4cc9f0;
	}

	.toolbar-actions,
	.selection-actions,
	.batch-bar {
		gap: 7px;
	}

	button {
		font: inherit;
	}

	button:disabled {
		cursor: not-allowed;
		opacity: 0.42;
	}

	.btn-secondary,
	.batch-bar button {
		height: 32px;
		padding: 0 10px;
		border: 1px solid #46545f;
		border-radius: 4px;
		background: #24313a;
		color: #e4e9ed;
		cursor: pointer;
	}

	.btn-secondary:hover:not(:disabled),
	.batch-bar button:hover:not(:disabled) {
		border-color: #4cc9f0;
		background: #2d3c47;
	}

	.btn-link {
		padding: 4px;
		border: 0;
		background: transparent;
		color: #4cc9f0;
		cursor: pointer;
		white-space: nowrap;
	}

	.status-banner,
	.filter-summary {
		min-height: 30px;
		box-sizing: border-box;
		margin-bottom: 9px;
		padding: 6px 9px;
		border: 1px solid #33414b;
		border-radius: 4px;
		background: #111920;
	}

	.status-banner {
		font-size: 0.8rem;
		color: #b6c0c8;
	}

	.status-banner span {
		display: inline-block;
		width: 8px;
		height: 8px;
		margin-right: 8px;
		border-radius: 50%;
		background: #75828c;
	}

	.status-banner span.working {
		background: #ffb02e;
	}

	.status-banner.warning,
	.inline-error {
		color: #ffb02e;
	}

	.filter-summary {
		gap: 14px;
		white-space: nowrap;
	}

	.export-status {
		min-width: 0;
		margin-left: auto;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.viewer-layout {
		display: grid;
		grid-template-columns: minmax(165px, 0.72fr) minmax(190px, 0.9fr) minmax(285px, 2.3fr);
		flex: 1;
		gap: 10px;
		min-height: 0;
	}

	.contacts-pane,
	.threads-pane,
	.chat-pane {
		display: flex;
		min-width: 0;
		min-height: 0;
		flex-direction: column;
		overflow: hidden;
		border: 1px solid #2f3d47;
		border-radius: 4px;
		background: #0f161c;
	}

	.pane-header,
	.chat-header {
		box-sizing: border-box;
		min-height: 58px;
		justify-content: space-between;
		gap: 8px;
		padding: 10px;
		border-bottom: 1px solid #2f3d47;
		background: #172129;
	}

	.pane-header > div,
	.chat-header > div {
		display: flex;
		min-width: 0;
		flex-direction: column;
		gap: 3px;
	}

	.pane-header input {
		width: min(54%, 180px);
	}

	.pane-header.compact {
		justify-content: flex-start;
	}

	.virtual-list,
	.messages-viewport {
		position: relative;
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		overscroll-behavior: contain;
		scrollbar-gutter: stable;
	}

	.contact-row {
		display: flex;
		box-sizing: border-box;
		align-items: center;
		gap: 9px;
		padding: 7px 9px;
		border-bottom: 1px solid #25323b;
		cursor: pointer;
	}

	.contact-row:hover,
	.contact-row.selected,
	.thread-row:hover,
	.thread-row.selected {
		background: #1c2b34;
	}

	.contact-row.selected,
	.thread-row.selected {
		box-shadow: inset 3px 0 #4cc9f0;
	}

	.contact-row input {
		width: 16px;
		height: 16px;
		margin: 0;
		accent-color: #4cc9f0;
	}

	.contact-copy {
		display: flex;
		min-width: 0;
		flex-direction: column;
		gap: 3px;
	}

	.contact-copy strong,
	.contact-copy span,
	.thread-row strong,
	.thread-preview,
	.thread-time {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.contact-copy strong,
	.thread-row strong {
		font-size: 0.8rem;
	}

	.thread-row {
		display: flex;
		box-sizing: border-box;
		width: 100%;
		padding: 9px 10px;
		flex-direction: column;
		gap: 4px;
		border: 0;
		border-bottom: 1px solid #25323b;
		background: transparent;
		color: #e4e9ed;
		text-align: left;
		cursor: pointer;
	}

	.thread-topline {
		width: 100%;
		justify-content: space-between;
		gap: 8px;
	}

	.thread-topline strong {
		min-width: 0;
	}

	.thread-topline > span {
		padding: 1px 5px;
		border-radius: 3px;
		background: #2b3943;
		color: #b9c2c9;
		font-size: 0.68rem;
	}

	.inline-error {
		padding: 8px 10px;
		border-bottom: 1px solid #4b3a25;
		background: #241d15;
		font-size: 0.76rem;
		word-break: break-word;
	}

	.chat-header h4 {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.selection-actions {
		flex-shrink: 0;
		font-size: 0.75rem;
	}

	.batch-bar {
		box-sizing: border-box;
		min-height: 42px;
		padding: 5px 9px;
		flex-wrap: wrap;
		border-bottom: 1px solid #2f3d47;
		background: #111920;
	}

	.batch-bar button {
		height: 28px;
		font-size: 0.72rem;
	}

	.batch-bar button.danger {
		border-color: #d38a28;
		color: #ffb02e;
	}

	.batch-bar > span {
		min-width: 0;
		margin-left: auto;
		overflow: hidden;
		color: #8e9aa4;
		font-size: 0.72rem;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.messages-viewport {
		padding: 0 12px;
	}

	.message-slot {
		display: flex;
		box-sizing: border-box;
		align-items: flex-start;
		gap: 8px;
		padding: 10px 0;
	}

	.message-slot.sent {
		flex-direction: row-reverse;
	}

	.message-checkbox {
		display: flex;
		padding-top: 5px;
		align-items: center;
	}

	.message-checkbox input {
		width: 16px;
		height: 16px;
		margin: 0;
		accent-color: #4cc9f0;
	}

	.bubble {
		display: flex;
		box-sizing: border-box;
		max-width: min(76%, 760px);
		max-height: 150px;
		padding: 9px 11px;
		flex-direction: column;
		gap: 6px;
		overflow: auto;
		border-radius: 8px 8px 8px 2px;
		background: #273640;
		color: #edf1f4;
	}

	.sent .bubble {
		border-radius: 8px 8px 2px 8px;
		background: #c8edf7;
		color: #0f1a20;
	}

	.text {
		line-height: 1.35;
		font-size: 0.84rem;
		white-space: pre-wrap;
		word-break: break-word;
	}

	.message-meta {
		display: flex;
		gap: 8px;
		flex-wrap: wrap;
	}

	.sent .message-meta {
		color: #40535e;
	}

	.attachments {
		display: flex;
		gap: 5px;
		flex-wrap: wrap;
	}

	.attachments button {
		display: flex;
		min-width: 0;
		max-width: 240px;
		padding: 4px 7px;
		align-items: center;
		gap: 6px;
		border: 1px solid #52616b;
		border-radius: 3px;
		background: rgba(8, 14, 18, 0.45);
		color: inherit;
		cursor: pointer;
	}

	.attachments button span {
		overflow: hidden;
		font-size: 0.72rem;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.attachments button small {
		opacity: 0.68;
	}

	.list-loading,
	.end-marker {
		padding: 9px;
		color: #8e9aa4;
		font-size: 0.75rem;
		text-align: center;
	}

	.messages-loading,
	.end-marker {
		height: 34px;
		box-sizing: border-box;
	}

	.empty {
		margin: auto;
		padding: 30px;
		color: #8e9aa4;
		font-size: 0.82rem;
		text-align: center;
	}

	.empty.small {
		margin: 0;
		padding: 20px 10px;
	}

	@media (max-width: 1100px) {
		.viewer-layout {
			grid-template-columns: minmax(155px, 0.65fr) minmax(180px, 0.8fr) minmax(280px, 1.7fr);
		}

		.message-search {
			min-width: 220px;
		}
	}

	@media (max-width: 820px) {
		.viewer-toolbar {
			align-items: stretch;
			flex-wrap: wrap;
		}

		.message-search {
			min-width: 100%;
			order: 3;
		}

		.viewer-layout {
			grid-template-columns: 1fr 1fr;
			grid-template-rows: minmax(170px, 0.65fr) minmax(300px, 1.35fr);
		}

		.chat-pane {
			grid-column: 1 / -1;
		}
	}

	@media (max-width: 640px) {
		.viewer-toolbar {
			flex-direction: column;
			align-items: stretch;
			gap: 8px;
		}

		.title-group,
		.toolbar-actions {
			justify-content: space-between;
		}

		.message-search {
			min-width: 100%;
			max-width: none;
			order: 0;
		}

		.filter-summary {
			flex-wrap: wrap;
			white-space: normal;
			gap: 8px;
		}

		.filter-summary > span {
			white-space: nowrap;
		}

		.export-status {
			width: 100%;
			margin-left: 0;
		}

		.viewer-layout {
			display: flex;
			flex-direction: column;
			gap: 8px;
		}

		.contacts-pane,
		.threads-pane {
			min-height: 180px;
			max-height: 220px;
		}

		.chat-pane {
			min-height: 260px;
			flex: 1;
		}

		.pane-header input {
			width: 100%;
		}

		.chat-header {
			flex-direction: column;
			align-items: flex-start;
			gap: 8px;
		}

		.selection-actions {
			flex-wrap: wrap;
		}

		.batch-bar {
			gap: 5px;
		}

		.batch-bar button {
			font-size: 0.7rem;
			padding: 0 8px;
		}

		.bubble {
			max-width: 88%;
		}

		.message-slot {
			gap: 6px;
		}

		.attachments button {
			max-width: 100%;
		}
	}
</style>
