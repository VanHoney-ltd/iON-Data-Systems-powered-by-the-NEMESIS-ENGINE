package main

// AgentCatalog describes one forensic agent for the UI.
type AgentCatalog struct {
	ID              string `json:"id"`
	Name            string `json:"name"`
	Description     string `json:"description"`
	Scope           string `json:"scope"`
	EstimatedTime   string `json:"estimatedTime"`
	Risk            string `json:"risk"`
	Tip             string `json:"tip"`
	RequiresDecryption bool `json:"requiresDecryption"`
}

// agentCatalog is the single source of truth for what agents the UI exposes.
// Descriptions and tips are shown as tooltips / help text.
var agentCatalog = []AgentCatalog{
	{
		ID:              "vigil",
		Name:            "Vigil",
		Description:     "System-level artifact extraction",
		Scope:           "Device info, installed apps, system logs, usage metadata",
		EstimatedTime:   "Fast (seconds to minutes)",
		Risk:            "Safe to run first",
		Tip:             "Good starting agent. Reads mostly small plist and SQLite files.",
		RequiresDecryption: true,
	},
	{
		ID:              "cerberus",
		Name:            "Cerberus",
		Description:     "SMS, MMS, calls, voicemail, contacts",
		Scope:           "Messages, call history, voicemail metadata, contacts",
		EstimatedTime:   "Moderate (minutes)",
		Risk:            "Reads personal communication data",
		Tip:             "Works against the sms.db, call_history.db, and Contacts databases.",
		RequiresDecryption: true,
	},
	{
		ID:              "charon",
		Name:            "Charon",
		Description:     "Photo and media extraction",
		Scope:           "Camera roll, screenshots, saved photos, videos",
		EstimatedTime:   "Depends on media count (can be long on 70 GB+ backups)",
		Risk:            "Large output; lots of file I/O",
		Tip:             "Extracts media file paths and metadata. Does not transcode video.",
		RequiresDecryption: true,
	},
	{
		ID:              "echo",
		Name:            "Echo",
		Description:     "Audio evidence",
		Scope:           "Voicemail recordings, audio messages, voice memos",
		EstimatedTime:   "Moderate",
		Risk:            "May produce large audio files",
		Tip:             "Useful for voice memos and call voicemail exports.",
		RequiresDecryption: true,
	},
	{
		ID:              "hermes",
		Name:            "Hermes",
		Description:     "Media catalog, transcription & diarization",
		Scope:           "Cross-agent media catalog and transcription queue",
		EstimatedTime:   "Slow on large backups (tens of minutes)",
		Risk:            "Very large output; runs Whisper/transcription models",
		Tip:             "Best run after Charon. Skip on first pass if you just want quick results.",
		RequiresDecryption: true,
	},
	{
		ID:              "nyx",
		Name:            "Nyx",
		Description:     "Safari history, bookmarks, autofill",
		Scope:           "Browser history, bookmarks, autofill entries",
		EstimatedTime:   "Fast",
		Risk:            "Safe",
		Tip:             "Reads History.db and Bookmarks databases.",
		RequiresDecryption: true,
	},
	{
		ID:              "plutus",
		Name:            "Plutus",
		Description:     "Apple Wallet / financial data",
		Scope:           "Payment cards, transactions, passes",
		EstimatedTime:   "Fast",
		Risk:            "Financial data is sensitive",
		Tip:             "Looks for Passbook and financial SQLite stores.",
		RequiresDecryption: true,
	},
	{
		ID:              "atlas",
		Name:            "Atlas",
		Description:     "App-based location extraction",
		Scope:           "GPS coordinates from app databases and EXIF",
		EstimatedTime:   "Moderate",
		Risk:            "Location data is sensitive",
		Tip:             "Combines EXIF GPS from photos with location tables from apps.",
		RequiresDecryption: true,
	},
}

// GetAgentCatalog returns the list of available agents with UI metadata.
func (a *App) GetAgentCatalog() []AgentCatalog {
	return agentCatalog
}

// GetAgent returns metadata for a single agent, or nil if unknown.
func (a *App) GetAgent(id string) *AgentCatalog {
	for _, ag := range agentCatalog {
		if ag.ID == id {
			return &ag
		}
	}
	return nil
}
