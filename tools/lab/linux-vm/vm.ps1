<#
The Linux test machine from the Windows host: a VirtualBox VM (Ubuntu 24.04 desktop, GNOME on
Xorg) that runs headless. Everything goes through VBoxManage, so the host's own keyboard, mouse
and clipboard are never touched; keep the VM's clipboard sharing and drag-and-drop off.

  tools/lab/linux-vm/vm.ps1 configure               # shut down cleanly, apply the lab's hardware
  tools/lab/linux-vm/vm.ps1 start                   # boot headless, wait for the desktop
  tools/lab/linux-vm/vm.ps1 setup [-Branch <name>]  # copy the guest scripts to ~/lab and run setup.sh
                                                    # (long: packages, Rust, the first build)

In the guest (~/lab): build.sh <ref> [label] builds a branch, PR (pr/<n>) or commit with the lab's
vmtest profile into ~/lab/bin/<label>/ in a terminal on its desktop; measure.sh times a build's
start; streams.mjs and streams-chrome.mjs hand HLS and DASH players to MDM from the Snap Firefox
and Google Chrome (the extension's three paths, checked against the lab server's log);
use-system-helpers.sh makes MDM use the system's yt-dlp, ffmpeg and gallery-dl.
  tools/lab/linux-vm/vm.ps1 run "<bash command>"    # run in the desktop user's session
  tools/lab/linux-vm/vm.ps1 shot <file.png>         # screenshot of the VM's screen
  tools/lab/linux-vm/vm.ps1 get <guest path> <host folder>
  tools/lab/linux-vm/vm.ps1 put <host file> <guest path>
  tools/lab/linux-vm/vm.ps1 clone <new name> [-Snapshot <name>] [-Cpus 6] [-MemoryMb 10240]
                                                    # a linked clone of a snapshot of $MDM_VM, its own
                                                    # hostname; then `start` it with MDM_VM=<name>

Snapshots for clones are taken powered off (`clone` shuts the VM down first when the snapshot
doesn't exist yet): a live snapshot saves the whole memory, and under Hyper-V (VirtualBox's NEM
mode, "VT-x is not available") it stalled with an empty .sav file and had to be killed.

The VM's name, user and password come from MDM_VM, MDM_VM_USER and MDM_VM_PASSWORD (defaults:
mdm-ubuntu, mdm, and the lab's throwaway password). Commands run with the desktop session's
DISPLAY and D-Bus, so GUI programs open on the VM's screen.
#>
param(
    [Parameter(Position = 0, Mandatory)] [ValidateSet('configure', 'start', 'setup', 'run', 'shot', 'get', 'put', 'clone')] [string] $Action,
    [Parameter(Position = 1)] [string] $Arg1,
    [Parameter(Position = 2)] [string] $Arg2,
    [string] $Branch = 'main',
    [string] $Snapshot = 'base',
    [int] $Cpus = 6,
    [int] $MemoryMb = 10240
)
$ErrorActionPreference = 'Stop'
$vbox = Join-Path $env:ProgramFiles 'Oracle\VirtualBox\VBoxManage.exe'
$vm = if ($env:MDM_VM) { $env:MDM_VM } else { 'mdm-ubuntu' }
$user = if ($env:MDM_VM_USER) { $env:MDM_VM_USER } else { 'mdm' }
$pass = if ($env:MDM_VM_PASSWORD) { $env:MDM_VM_PASSWORD } else { 'mdm123456789' }
$auth = @('--username', $user, '--password', $pass)

function Invoke-Guest([string] $Command) {
    # The desktop session's display and bus, so GUI programs and gsettings reach it.
    $session = 'export DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus; ' +
        'export XAUTHORITY=$(ls /run/user/$(id -u)/gdm/Xauthority 2>/dev/null || echo $HOME/.Xauthority); ' +
        "export MDM_SUDO_PASSWORD='$pass'; [ -f ~/.cargo/env ] && . ~/.cargo/env; "
    & $vbox guestcontrol $vm run --exe /bin/bash @auth --wait-stdout --wait-stderr -- -c ($session + $Command)
    if ($LASTEXITCODE) { throw "guest command failed ($LASTEXITCODE)" }
}

switch ($Action) {
    'configure' {
        # Settings only change on a powered-off machine: an ACPI shutdown first, as a person would.
        $running = { (& $vbox showvminfo $vm --machinereadable | Select-String '^VMState=').ToString() -match 'running' }
        if (& $running) {
            & $vbox controlvm $vm acpipowerbutton
            for ($i = 0; $i -lt 90 -and (& $running); $i++) { Start-Sleep 2 }
            if (& $running) { throw "$vm did not shut down" }
        }
        # A USB tablet, so a viewer's pointer moves in and out without being captured. 8 CPUs,
        # 12 GB and 3D (VMSVGA): GNOME and MDM's OpenGL drawn by the host's GPU rather than
        # llvmpipe (VirtualBox may run on Hyper-V here, which is slower). Clipboard and
        # drag-and-drop stay off: nothing crosses between the lab and the host.
        & $vbox modifyvm $vm --usb-ohci on --mouse usbtablet --cpus 8 --memory 12288 `
            --graphicscontroller vmsvga --accelerate-3d on --vram 256 `
            --clipboard-mode disabled --drag-and-drop disabled
        if ($LASTEXITCODE) { throw "modifyvm failed ($LASTEXITCODE)" }
    }
    'start' {
        $state = (& $vbox showvminfo $vm --machinereadable | Select-String '^VMState=').ToString()
        if ($state -notmatch 'running') { & $vbox startvm $vm --type headless | Out-Null }
        # Ready when the Guest Additions run (guestcontrol needs them) and the user is signed in.
        for ($i = 0; $i -lt 90; $i++) {
            $level = & $vbox showvminfo $vm --machinereadable | Select-String '^GuestAdditionsRunLevel=[23]'
            $users = & $vbox guestproperty get $vm /VirtualBox/GuestInfo/OS/LoggedInUsersList 2>$null
            if ($level -and "$users" -match "\b$user\b") { break }
            Start-Sleep 2
        }
        Start-Sleep 10
        Invoke-Guest 'loginctl list-sessions --no-legend | grep seat0'
    }
    'setup' {
        Invoke-Guest 'mkdir -p ~/lab'
        # The guest side: setup, the build, the harnesses (everything here but this script).
        $files = Get-ChildItem $PSScriptRoot -File | Where-Object { $_.Extension -in '.sh', '.mjs' } | ForEach-Object FullName
        & $vbox guestcontrol $vm copyto @auth --target-directory /home/$user/lab/ @files
        Invoke-Guest "bash ~/lab/setup.sh --branch '$Branch'"
    }
    'clone' {
        # A linked clone shares the snapshot's disk and writes only its own changes: small, and
        # it has the toolchain, the checkout and a warm target folder already.
        $name = $Arg1
        if (-not $name) { throw 'clone: a name for the new VM' }
        $snaps = & $vbox snapshot $vm list --machinereadable 2>$null
        if (-not ($snaps | Select-String "=`"$Snapshot`"")) {
            $running = { (& $vbox showvminfo $vm --machinereadable | Select-String '^VMState=').ToString() -match 'running' }
            if (& $running) {
                & $vbox controlvm $vm acpipowerbutton
                for ($i = 0; $i -lt 90 -and (& $running); $i++) { Start-Sleep 2 }
                if (& $running) { throw "$vm did not shut down" }
            }
            & $vbox snapshot $vm take $Snapshot --description 'Lab base for linked clones (tools/lab/linux-vm)'
        }
        & $vbox clonevm $vm --snapshot $Snapshot --options link --name $name --register
        if ($LASTEXITCODE) { throw "clonevm failed ($LASTEXITCODE)" }
        # The lab's hardware rules, with its own size; clipboard and drag-and-drop stay off.
        & $vbox modifyvm $name --cpus $Cpus --memory $MemoryMb --usb-ohci on --mouse usbtablet `
            --graphicscontroller vmsvga --accelerate-3d on --vram 256 `
            --clipboard-mode disabled --drag-and-drop disabled
        & $vbox startvm $name --type headless | Out-Null
        for ($i = 0; $i -lt 90; $i++) {
            $level = & $vbox showvminfo $name --machinereadable | Select-String '^GuestAdditionsRunLevel=[23]'
            if ($level) { break }
            Start-Sleep 2
        }
        Start-Sleep 10
        & $vbox guestcontrol $name run --exe /bin/bash @auth --wait-stdout --wait-stderr -- -c `
            "echo '$pass' | sudo -S -p '' hostnamectl set-hostname $name && echo '$pass' | sudo -S -p '' sed -i 's/$vm/$name/g' /etc/hosts && hostname"
    }
    'run' { Invoke-Guest $Arg1 }
    'shot' { & $vbox controlvm $vm screenshotpng ([IO.Path]::GetFullPath($Arg1)) }
    'get' { & $vbox guestcontrol $vm copyfrom @auth --target-directory $Arg2 $Arg1 }
    'put' { & $vbox guestcontrol $vm copyto @auth --target-directory $Arg2 $Arg1 }
}
