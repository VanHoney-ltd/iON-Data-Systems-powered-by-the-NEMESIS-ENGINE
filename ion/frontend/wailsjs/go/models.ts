export namespace main {
	
	export class AgentCatalog {
	    id: string;
	    name: string;
	    description: string;
	    scope: string;
	    estimatedTime: string;
	    risk: string;
	    tip: string;
	    requiresDecryption: boolean;
	
	    static createFrom(source: any = {}) {
	        return new AgentCatalog(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.id = source["id"];
	        this.name = source["name"];
	        this.description = source["description"];
	        this.scope = source["scope"];
	        this.estimatedTime = source["estimatedTime"];
	        this.risk = source["risk"];
	        this.tip = source["tip"];
	        this.requiresDecryption = source["requiresDecryption"];
	    }
	}
	export class AppDataHit {
	    bundle_id: string;
	    app_name: string;
	    file_path: string;
	    line: number;
	    snippet: string;
	    match_type: string;
	
	    static createFrom(source: any = {}) {
	        return new AppDataHit(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.bundle_id = source["bundle_id"];
	        this.app_name = source["app_name"];
	        this.file_path = source["file_path"];
	        this.line = source["line"];
	        this.snippet = source["snippet"];
	        this.match_type = source["match_type"];
	    }
	}
	export class AppFile {
	    path: string;
	    name: string;
	    size: number;
	    dir: boolean;
	
	    static createFrom(source: any = {}) {
	        return new AppFile(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.path = source["path"];
	        this.name = source["name"];
	        this.size = source["size"];
	        this.dir = source["dir"];
	    }
	}
	export class CaseSummary {
	    name: string;
	    rootPath: string;
	    backupPath: string;
	    backupSizeBytes: number;
	    backupSize: string;
	    evidenceAgents: string[];
	    lastRunStatus: string;
	    hasBackup: boolean;
	
	    static createFrom(source: any = {}) {
	        return new CaseSummary(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.name = source["name"];
	        this.rootPath = source["rootPath"];
	        this.backupPath = source["backupPath"];
	        this.backupSizeBytes = source["backupSizeBytes"];
	        this.backupSize = source["backupSize"];
	        this.evidenceAgents = source["evidenceAgents"];
	        this.lastRunStatus = source["lastRunStatus"];
	        this.hasBackup = source["hasBackup"];
	    }
	}
	export class DeviceInfo {
	    udid: string;
	    device_name: string;
	    product_type: string;
	    product_version: string;
	    serial_number: string;
	    imei: string;
	    imei1: string;
	    imei2: string;
	    iccid: string;
	    phone_number: string;
	    build_version: string;
	    model_number: string;
	    model_name: string;
	    chipset: string;
	    wifi_address: string;
	    bluetooth_address: string;
	    raw_fields: Record<string, string>;
	
	    static createFrom(source: any = {}) {
	        return new DeviceInfo(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.udid = source["udid"];
	        this.device_name = source["device_name"];
	        this.product_type = source["product_type"];
	        this.product_version = source["product_version"];
	        this.serial_number = source["serial_number"];
	        this.imei = source["imei"];
	        this.imei1 = source["imei1"];
	        this.imei2 = source["imei2"];
	        this.iccid = source["iccid"];
	        this.phone_number = source["phone_number"];
	        this.build_version = source["build_version"];
	        this.model_number = source["model_number"];
	        this.model_name = source["model_name"];
	        this.chipset = source["chipset"];
	        this.wifi_address = source["wifi_address"];
	        this.bluetooth_address = source["bluetooth_address"];
	        this.raw_fields = source["raw_fields"];
	    }
	}
	export class ConnectedDeviceStatus {
	    udid: string;
	    pairing_state: string;
	    pairing_detail: string;
	    trust_refresh_detail: string;
	    suggested_mount_point: string;
	    active_mounts: string[];
	    device_info?: DeviceInfo;
	    info_error: string;
	
	    static createFrom(source: any = {}) {
	        return new ConnectedDeviceStatus(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.udid = source["udid"];
	        this.pairing_state = source["pairing_state"];
	        this.pairing_detail = source["pairing_detail"];
	        this.trust_refresh_detail = source["trust_refresh_detail"];
	        this.suggested_mount_point = source["suggested_mount_point"];
	        this.active_mounts = source["active_mounts"];
	        this.device_info = this.convertValues(source["device_info"], DeviceInfo);
	        this.info_error = source["info_error"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	
	export class ToolStatus {
	    name: string;
	    available: boolean;
	    detail: string;
	
	    static createFrom(source: any = {}) {
	        return new ToolStatus(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.name = source["name"];
	        this.available = source["available"];
	        this.detail = source["detail"];
	    }
	}
	export class DeviceStatus {
	    connected_devices: ConnectedDeviceStatus[];
	    paired_count: number;
	    mount_root: string;
	    tool_status: ToolStatus[];
	
	    static createFrom(source: any = {}) {
	        return new DeviceStatus(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.connected_devices = this.convertValues(source["connected_devices"], ConnectedDeviceStatus);
	        this.paired_count = source["paired_count"];
	        this.mount_root = source["mount_root"];
	        this.tool_status = this.convertValues(source["tool_status"], ToolStatus);
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	export class EvidenceSummary {
	    agent: string;
	    slug: string;
	    schema_version: number;
	    generated_at: string;
	    record_count: number;
	    evidence_dir: string;
	
	    static createFrom(source: any = {}) {
	        return new EvidenceSummary(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.agent = source["agent"];
	        this.slug = source["slug"];
	        this.schema_version = source["schema_version"];
	        this.generated_at = source["generated_at"];
	        this.record_count = source["record_count"];
	        this.evidence_dir = source["evidence_dir"];
	    }
	}
	export class InstalledApp {
	    bundle_id: string;
	    name: string;
	    domain: string;
	    data_size_bytes: number;
	    data_size: string;
	    container_path: string;
	    group_containers: string[];
	    has_documents: boolean;
	    has_library: boolean;
	    file_count: number;
	    source: string;
	
	    static createFrom(source: any = {}) {
	        return new InstalledApp(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.bundle_id = source["bundle_id"];
	        this.name = source["name"];
	        this.domain = source["domain"];
	        this.data_size_bytes = source["data_size_bytes"];
	        this.data_size = source["data_size"];
	        this.container_path = source["container_path"];
	        this.group_containers = source["group_containers"];
	        this.has_documents = source["has_documents"];
	        this.has_library = source["has_library"];
	        this.file_count = source["file_count"];
	        this.source = source["source"];
	    }
	}
	export class MessageActionResult {
	    action: string;
	    count: number;
	    path?: string;
	    message?: string;
	
	    static createFrom(source: any = {}) {
	        return new MessageActionResult(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.action = source["action"];
	        this.count = source["count"];
	        this.path = source["path"];
	        this.message = source["message"];
	    }
	}
	export class MessageContactSummary {
	    handle: string;
	    normalizedHandle: string;
	    displayName: string;
	    messageCount: number;
	    threadCount: number;
	    firstTimestampUtc?: string;
	    lastTimestampUtc?: string;
	
	    static createFrom(source: any = {}) {
	        return new MessageContactSummary(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.handle = source["handle"];
	        this.normalizedHandle = source["normalizedHandle"];
	        this.displayName = source["displayName"];
	        this.messageCount = source["messageCount"];
	        this.threadCount = source["threadCount"];
	        this.firstTimestampUtc = source["firstTimestampUtc"];
	        this.lastTimestampUtc = source["lastTimestampUtc"];
	    }
	}
	export class MessageContactPage {
	    contacts: MessageContactSummary[];
	    total: number;
	    limit: number;
	    offset: number;
	    hasMore: boolean;
	
	    static createFrom(source: any = {}) {
	        return new MessageContactPage(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.contacts = this.convertValues(source["contacts"], MessageContactSummary);
	        this.total = source["total"];
	        this.limit = source["limit"];
	        this.offset = source["offset"];
	        this.hasMore = source["hasMore"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	
	export class MessageExportResult {
	    path: string;
	    count: number;
	    format: string;
	
	    static createFrom(source: any = {}) {
	        return new MessageExportResult(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.path = source["path"];
	        this.count = source["count"];
	        this.format = source["format"];
	    }
	}
	export class MessageIndexStatus {
	    caseName: string;
	    state: string;
	    sourcePath: string;
	    indexPath: string;
	    schemaVersion: number;
	    sourceSizeBytes: number;
	    sourceModifiedAt: string;
	    processedMessages: number;
	    messageCount: number;
	    threadCount: number;
	    indexedAt?: string;
	    reused: boolean;
	    error?: string;
	
	    static createFrom(source: any = {}) {
	        return new MessageIndexStatus(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.caseName = source["caseName"];
	        this.state = source["state"];
	        this.sourcePath = source["sourcePath"];
	        this.indexPath = source["indexPath"];
	        this.schemaVersion = source["schemaVersion"];
	        this.sourceSizeBytes = source["sourceSizeBytes"];
	        this.sourceModifiedAt = source["sourceModifiedAt"];
	        this.processedMessages = source["processedMessages"];
	        this.messageCount = source["messageCount"];
	        this.threadCount = source["threadCount"];
	        this.indexedAt = source["indexedAt"];
	        this.reused = source["reused"];
	        this.error = source["error"];
	    }
	}
	export class MessageRecord {
	    _id?: string;
	    thread_id: string;
	    chat_id?: number;
	    chat_identifier?: string;
	    chat_display_name?: string;
	    message_id: number;
	    guid: string;
	    timestamp_raw?: number;
	    timestamp_utc?: string;
	    direction: string;
	    handle: string;
	    service: string;
	    text?: string;
	    subject?: string;
	    attachment_count: number;
	    attachment_paths: string[];
	    attachment_export_paths: string[];
	    attachment_mime_types: string[];
	    is_read: boolean;
	    is_delivered: boolean;
	    is_sent: boolean;
	    is_audio_message: boolean;
	    item_type?: number;
	    group_title?: string;
	    reply_to_guid?: string;
	    balloon_bundle_id?: string;
	
	    static createFrom(source: any = {}) {
	        return new MessageRecord(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this._id = source["_id"];
	        this.thread_id = source["thread_id"];
	        this.chat_id = source["chat_id"];
	        this.chat_identifier = source["chat_identifier"];
	        this.chat_display_name = source["chat_display_name"];
	        this.message_id = source["message_id"];
	        this.guid = source["guid"];
	        this.timestamp_raw = source["timestamp_raw"];
	        this.timestamp_utc = source["timestamp_utc"];
	        this.direction = source["direction"];
	        this.handle = source["handle"];
	        this.service = source["service"];
	        this.text = source["text"];
	        this.subject = source["subject"];
	        this.attachment_count = source["attachment_count"];
	        this.attachment_paths = source["attachment_paths"];
	        this.attachment_export_paths = source["attachment_export_paths"];
	        this.attachment_mime_types = source["attachment_mime_types"];
	        this.is_read = source["is_read"];
	        this.is_delivered = source["is_delivered"];
	        this.is_sent = source["is_sent"];
	        this.is_audio_message = source["is_audio_message"];
	        this.item_type = source["item_type"];
	        this.group_title = source["group_title"];
	        this.reply_to_guid = source["reply_to_guid"];
	        this.balloon_bundle_id = source["balloon_bundle_id"];
	    }
	}
	export class MessagePage {
	    messages: MessageRecord[];
	    total: number;
	    limit: number;
	    offset: number;
	    nextOffset: number;
	    hasMore: boolean;
	
	    static createFrom(source: any = {}) {
	        return new MessagePage(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.messages = this.convertValues(source["messages"], MessageRecord);
	        this.total = source["total"];
	        this.limit = source["limit"];
	        this.offset = source["offset"];
	        this.nextOffset = source["nextOffset"];
	        this.hasMore = source["hasMore"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	export class MessageQuery {
	    handles: string[];
	    threadId: string;
	    search: string;
	    limit: number;
	    offset: number;
	
	    static createFrom(source: any = {}) {
	        return new MessageQuery(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.handles = source["handles"];
	        this.threadId = source["threadId"];
	        this.search = source["search"];
	        this.limit = source["limit"];
	        this.offset = source["offset"];
	    }
	}
	
	export class MessageThreadSummary {
	    threadId: string;
	    chatId?: number;
	    chatIdentifier?: string;
	    displayName?: string;
	    messageCount: number;
	    attachmentCount: number;
	    firstTimestampUtc?: string;
	    lastTimestampUtc?: string;
	    participants: string[];
	    participantNames: string[];
	    htmlPath: string;
	
	    static createFrom(source: any = {}) {
	        return new MessageThreadSummary(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.threadId = source["threadId"];
	        this.chatId = source["chatId"];
	        this.chatIdentifier = source["chatIdentifier"];
	        this.displayName = source["displayName"];
	        this.messageCount = source["messageCount"];
	        this.attachmentCount = source["attachmentCount"];
	        this.firstTimestampUtc = source["firstTimestampUtc"];
	        this.lastTimestampUtc = source["lastTimestampUtc"];
	        this.participants = source["participants"];
	        this.participantNames = source["participantNames"];
	        this.htmlPath = source["htmlPath"];
	    }
	}
	export class MessageThreadPage {
	    threads: MessageThreadSummary[];
	    total: number;
	    limit: number;
	    offset: number;
	    hasMore: boolean;
	
	    static createFrom(source: any = {}) {
	        return new MessageThreadPage(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.threads = this.convertValues(source["threads"], MessageThreadSummary);
	        this.total = source["total"];
	        this.limit = source["limit"];
	        this.offset = source["offset"];
	        this.hasMore = source["hasMore"];
	    }
	
		convertValues(a: any, classs: any, asMap: boolean = false): any {
		    if (!a) {
		        return a;
		    }
		    if (a.slice && a.map) {
		        return (a as any[]).map(elem => this.convertValues(elem, classs));
		    } else if ("object" === typeof a) {
		        if (asMap) {
		            for (const key of Object.keys(a)) {
		                a[key] = new classs(a[key]);
		            }
		            return a;
		        }
		        return new classs(a);
		    }
		    return a;
		}
	}
	

}

