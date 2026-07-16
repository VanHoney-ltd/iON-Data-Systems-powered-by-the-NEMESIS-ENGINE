package main

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

// DeviceInfo mirrors the Rust DeviceInfo struct for the UI.
type DeviceInfo struct {
	UDID             string            `json:"udid"`
	DeviceName       string            `json:"device_name"`
	ProductType      string            `json:"product_type"`
	ProductVersion   string            `json:"product_version"`
	SerialNumber     string            `json:"serial_number"`
	IMEI             string            `json:"imei"`
	IMEI1            string            `json:"imei1"`
	IMEI2            string            `json:"imei2"`
	ICCID            string            `json:"iccid"`
	PhoneNumber      string            `json:"phone_number"`
	BuildVersion     string            `json:"build_version"`
	ModelNumber      string            `json:"model_number"`
	ModelName        string            `json:"model_name"`
	Chipset          string            `json:"chipset"`
	WiFiAddress      string            `json:"wifi_address"`
	BluetoothAddress string            `json:"bluetooth_address"`
	RawFields        map[string]string `json:"raw_fields"`
}

// ConnectedDeviceStatus describes one connected iOS device.
type ConnectedDeviceStatus struct {
	UDID                string      `json:"udid"`
	PairingState        string      `json:"pairing_state"`
	PairingDetail       string      `json:"pairing_detail"`
	TrustRefreshDetail  string      `json:"trust_refresh_detail"`
	SuggestedMountPoint string      `json:"suggested_mount_point"`
	ActiveMounts        []string    `json:"active_mounts"`
	DeviceInfo          *DeviceInfo `json:"device_info"`
	InfoError           string      `json:"info_error"`
}

// ToolStatus reports whether a required CLI tool is available.
type ToolStatus struct {
	Name      string `json:"name"`
	Available bool   `json:"available"`
	Detail    string `json:"detail"`
}

// DeviceStatus is the full live-device overview.
type DeviceStatus struct {
	ConnectedDevices []ConnectedDeviceStatus `json:"connected_devices"`
	PairedCount      int                     `json:"paired_count"`
	MountRoot        string                  `json:"mount_root"`
	ToolStatus       []ToolStatus            `json:"tool_status"`
}

// GetDeviceStatus probes for connected iOS devices and tool availability.
// All external commands are bounded by a short timeout so the UI never freezes.
func (a *App) GetDeviceStatus() (DeviceStatus, error) {
	return getDeviceStatus(false)
}

// RefreshDeviceTrust unpairs each connected iOS device, triggers Apple's trust
// pairing dialogue, validates the new pair record, then returns fresh status.
func (a *App) RefreshDeviceTrust() (DeviceStatus, error) {
	return getDeviceStatus(true)
}

func getDeviceStatus(refreshTrust bool) (DeviceStatus, error) {
	tools := []string{"idevice_id", "idevicepair", "ideviceinfo", "ifuse", "fusermount"}
	var toolStatus []ToolStatus
	for _, name := range tools {
		detail := "not installed or not on PATH"
		available := false
		if path, err := exec.LookPath(name); err == nil {
			available = true
			detail = path
		}
		toolStatus = append(toolStatus, ToolStatus{Name: name, Available: available, Detail: detail})
	}

	mountRoot := filepath.Join(mustTempDir(), "ion-live-mounts")
	_ = os.MkdirAll(mountRoot, 0755)
	activeMounts := discoverActiveMounts(mountRoot)

	// Bound the whole device-discovery phase to 8 seconds.
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()

	trustResults := make(map[string]string)
	if refreshTrust {
		refreshCtx, refreshCancel := context.WithTimeout(context.Background(), 75*time.Second)
		defer refreshCancel()
		trustResults = refreshTrustForConnectedDevices(refreshCtx)
	}

	devices, err := discoverDevices(ctx, mountRoot, activeMounts, trustResults)
	if err != nil {
		return DeviceStatus{
			ConnectedDevices: []ConnectedDeviceStatus{},
			ToolStatus:       toolStatus,
			MountRoot:        mountRoot,
		}, err
	}

	pairedCount := 0
	for _, d := range devices {
		if d.PairingState == "paired" {
			pairedCount++
		}
	}

	return DeviceStatus{
		ConnectedDevices: devices,
		PairedCount:      pairedCount,
		MountRoot:        mountRoot,
		ToolStatus:       toolStatus,
	}, nil
}

// PairDevice pairs with the connected iOS device (or a specific UDID if provided).
func (a *App) PairDevice(udid string) error {
	args := []string{"pair"}
	if strings.TrimSpace(udid) != "" {
		args = append(args, "-u", udid)
	}
	out, err := runWithTimeout(15*time.Second, "idevicepair", args...)
	if err != nil {
		return fmt.Errorf("%s", string(out))
	}
	return nil
}

// UnpairDevice removes this computer's pairing record for the connected device.
func (a *App) UnpairDevice(udid string) error {
	args := []string{"unpair"}
	if strings.TrimSpace(udid) != "" {
		args = append(args, "-u", udid)
	}
	out, err := runWithTimeout(15*time.Second, "idevicepair", args...)
	if err != nil {
		return fmt.Errorf("%s", string(out))
	}
	return nil
}

// ValidatePairing checks whether the device pairing is still valid.
func (a *App) ValidatePairing(udid string) error {
	args := []string{"validate"}
	if strings.TrimSpace(udid) != "" {
		args = append(args, "-u", udid)
	}
	out, err := runWithTimeout(10*time.Second, "idevicepair", args...)
	if err != nil {
		return fmt.Errorf("%s", string(out))
	}
	return nil
}

// MountDevice mounts a paired device via ifuse.
func (a *App) MountDevice(udid string) (string, error) {
	if strings.TrimSpace(udid) == "" {
		return "", fmt.Errorf("UDID is required")
	}
	mountRoot := filepath.Join(mustTempDir(), "ion-live-mounts")
	mountPoint := filepath.Join(mountRoot, udid)
	_ = os.MkdirAll(mountPoint, 0755)

	out, err := runWithTimeout(15*time.Second, "ifuse", "-u", udid, mountPoint)
	if err != nil {
		return "", fmt.Errorf("%s", string(out))
	}
	return mountPoint, nil
}

// UnmountDevice unmounts a previously mounted device.
func (a *App) UnmountDevice(mountPoint string) error {
	if mountPoint == "" {
		return fmt.Errorf("mount point is required")
	}
	if _, err := runWithTimeout(10*time.Second, "fusermount", "-u", mountPoint); err == nil {
		return nil
	}
	if _, err := runWithTimeout(10*time.Second, "umount", mountPoint); err != nil {
		return fmt.Errorf("failed to unmount %s: %w", mountPoint, err)
	}
	return nil
}

func discoverDevices(ctx context.Context, mountRoot string, activeMounts []string, trustResults map[string]string) ([]ConnectedDeviceStatus, error) {
	out, err := runCtx(ctx, "idevice_id", "-l")
	if err != nil {
		return nil, fmt.Errorf("idevice_id failed: %w", err)
	}

	lines := strings.Split(string(out), "\n")
	results := make(chan ConnectedDeviceStatus, len(lines))
	var wg sync.WaitGroup

	for _, line := range lines {
		udid := strings.TrimSpace(line)
		if udid == "" {
			continue
		}
		wg.Add(1)
		go func(udid string) {
			defer wg.Done()
			// Each per-device probe is also bounded by the parent context,
			// but we cap individual calls at 3 seconds to keep the list responsive.
			probeCtx, cancel := context.WithTimeout(ctx, 3*time.Second)
			defer cancel()

			state, detail := pairingStateForUDID(probeCtx, udid)
			info, infoErr := deviceInfoForUDID(probeCtx, udid)
			suggested := filepath.Join(mountRoot, udid)

			results <- ConnectedDeviceStatus{
				UDID:                udid,
				PairingState:        state,
				PairingDetail:       detail,
				TrustRefreshDetail:  trustResults[udid],
				SuggestedMountPoint: suggested,
				ActiveMounts:        filterMounts(activeMounts, suggested),
				DeviceInfo:          info,
				InfoError:           infoErr,
			}
		}(udid)
	}

	go func() {
		wg.Wait()
		close(results)
	}()

	devices := []ConnectedDeviceStatus{}
	for d := range results {
		devices = append(devices, d)
	}
	return devices, nil
}

func refreshTrustForConnectedDevices(ctx context.Context) map[string]string {
	results := make(map[string]string)
	out, err := runCtx(ctx, "idevice_id", "-l")
	if err != nil {
		results[""] = fmt.Sprintf("device discovery failed before trust refresh: %s", commandDetail(out, err, ctx))
		return results
	}

	for _, line := range strings.Split(string(out), "\n") {
		udid := strings.TrimSpace(line)
		if udid == "" {
			continue
		}
		results[udid] = refreshTrustForUDID(ctx, udid)
	}
	return results
}

func refreshTrustForUDID(ctx context.Context, udid string) string {
	if out, err := runCtx(ctx, "idevicepair", "-u", udid, "unpair"); err != nil {
		detail := commandDetail(out, err, ctx)
		// A missing pair record is acceptable; the next pair call still forces
		// the device trust dialogue when the phone is unlocked.
		if !strings.Contains(strings.ToLower(detail), "not paired") {
			return "unpair failed: " + detail
		}
	}
	if out, err := runCtx(ctx, "idevicepair", "-u", udid, "pair"); err != nil {
		return "pair failed: " + commandDetail(out, err, ctx)
	}
	if out, err := runCtx(ctx, "idevicepair", "-u", udid, "validate"); err != nil {
		return "validate failed: " + commandDetail(out, err, ctx)
	}
	return "Trust refreshed and pairing validated."
}

func pairingStateForUDID(ctx context.Context, udid string) (string, string) {
	out, err := runCtx(ctx, "idevicepair", "-u", udid, "validate")
	if err != nil {
		return "unpaired", commandDetail(out, err, ctx)
	}
	return "paired", ""
}

func deviceInfoForUDID(ctx context.Context, udid string) (*DeviceInfo, string) {
	out, err := runCtx(ctx, "ideviceinfo", "-u", udid)
	if err != nil {
		return nil, commandDetail(out, err, ctx)
	}
	info, err := parseDeviceInfo(string(out))
	if err != nil {
		return nil, err.Error()
	}
	return info, ""
}

func parseDeviceInfo(infoStr string) (*DeviceInfo, error) {
	info := &DeviceInfo{RawFields: make(map[string]string)}
	for _, line := range strings.Split(infoStr, "\n") {
		key, value, ok := strings.Cut(line, ": ")
		if !ok {
			continue
		}
		key = strings.TrimSpace(key)
		value = strings.TrimSpace(value)
		info.RawFields[key] = value

		switch key {
		case "UniqueDeviceID":
			info.UDID = value
		case "DeviceName":
			info.DeviceName = value
		case "ProductType":
			info.ProductType = value
		case "ProductVersion":
			info.ProductVersion = value
		case "SerialNumber":
			info.SerialNumber = value
		case "InternationalMobileEquipmentIdentity":
			info.IMEI = value
		case "IntegratedCircuitCardIdentity":
			info.ICCID = value
		case "PhoneNumber":
			info.PhoneNumber = value
		case "BuildVersion":
			info.BuildVersion = value
		case "ModelNumber":
			info.ModelNumber = value
		case "ProductName", "DeviceClass":
			info.ModelName = value
		case "ChipID", "ChipSerialNo":
			info.Chipset = value
		case "WiFiAddress":
			info.WiFiAddress = value
		case "BluetoothAddress":
			info.BluetoothAddress = value
		case "IMEI":
			info.IMEI = value
		case "IMEI1":
			info.IMEI1 = value
		case "IMEI2":
			info.IMEI2 = value
		}
	}
	if info.UDID == "" {
		return nil, fmt.Errorf("could not parse UDID from ideviceinfo")
	}
	return info, nil
}

func commandDetail(out []byte, err error, ctx context.Context) string {
	detail := strings.TrimSpace(string(out))
	if detail != "" {
		return detail
	}
	if ctx.Err() != nil {
		return "probe timed out"
	}
	if err != nil {
		return err.Error()
	}
	return ""
}

func discoverActiveMounts(mountRoot string) []string {
	entries, err := os.ReadDir(mountRoot)
	if err != nil {
		return []string{}
	}
	mounts := []string{}
	for _, e := range entries {
		if e.IsDir() {
			mounts = append(mounts, filepath.Join(mountRoot, e.Name()))
		}
	}
	return mounts
}

func filterMounts(mounts []string, prefix string) []string {
	out := []string{}
	for _, m := range mounts {
		if m == prefix || strings.HasPrefix(m, prefix+string(filepath.Separator)) {
			out = append(out, m)
		}
	}
	return out
}

func mustTempDir() string {
	dir, err := os.UserHomeDir()
	if err != nil {
		dir = os.TempDir()
	}
	return dir
}

func runWithTimeout(timeout time.Duration, name string, args ...string) ([]byte, error) {
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	return runCtx(ctx, name, args...)
}

func runCtx(ctx context.Context, name string, args ...string) ([]byte, error) {
	cmd := exec.CommandContext(ctx, name, args...)
	return cmd.CombinedOutput()
}
