<#
.SYNOPSIS
  diskfull-vhd.ps1 -- a disposable small VHD that simulates "disk full"
  during niu installs/syncs (adversarial-matrix lane AM06).

.DESCRIPTION
  Creates a mountable NTFS volume of an exact small size (default 200 MB --
  the E:200MB sandbox shape), so a lane probe can drive `niu plugin add` /
  collection apply / wizard apply with a HOME that genuinely runs out of
  space mid-write. Deterministic, repeatable, and disposable: Remove
  detaches and deletes the VHD file, leaving the host volume untouched.

  Requires elevation ONCE at New time (diskpart). CI images create the VHD
  ahead of time and lanes reuse it with Status/Fill/Remove. No-admin
  fallback: use a VHD created in advance by an admin step, or skip the
  volume trick and simulate ENOSPC only at the filesystem API level
  (documented in scripts/adversarial/README.md).

  PowerShell 5.1 compatible. Stdlib only (diskpart + .NET FileStream).

.USAGE
  # create + mount a 200 MB volume, auto-picking a drive letter
  powershell -File scripts/adversarial/diskfull-vhd.ps1 New `
      -VhdPath target\adversarial-results\diskfull.vhd -SizeMB 200

  # fill a directory on that volume until ENOSPC; reports bytes written
  powershell -File scripts/adversarial/diskfull-vhd.ps1 Fill `
      -Path Q:\home -BlockSizeMB 4

  # status: mounted? free bytes?
  powershell -File scripts/adversarial/diskfull-vhd.ps1 Status `
      -VhdPath target\adversarial-results\diskfull.vhd

  # detach + delete
  powershell -File scripts/adversarial/diskfull-vhd.ps1 Remove `
      -VhdPath target\adversarial-results\diskfull.vhd

.EXIT CODES
  0 success; 3 not elevated / diskpart missing; 4 operation failed.
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0, Mandatory = $true)]
    [ValidateSet('New', 'Fill', 'Status', 'Remove')]
    [string]$Command,

    [string]$VhdPath = "",

    # New
    [int]$SizeMB = 200,
    [string]$MountLetter = "",   # auto-pick when empty

    # Fill
    [string]$Path = "",
    [int]$BlockSizeMB = 4
)

$ErrorActionPreference = 'Stop'

function Test-Elevated {
    $id = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object System.Security.Principal.WindowsPrincipal($id)).IsInRole(
        [System.Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-FreeLetter {
    $used = @(Get-PSDrive | ForEach-Object { "$($_.Name)" })
    foreach ($i in 72..90) {   # 'H'..'Z' by code (avoid low letters)
        $name = [string][char]$i
        if ($used -notcontains $name) { return $name }
    }
    throw "no free drive letter among H..Z"
}

function Get-AttachState([string]$path) {
    # "attached" in diskpart terms: some drive letter maps a volume whose
    # VHD backing file is $path. Cheap check: does diskpart list it?
    $script = [System.IO.Path]::GetTempFileName()
    Set-Content -Path $script -Value "list vdisk`r`n" -Encoding Ascii
    $out = & diskpart /s $script 2>&1 | Out-String
    Remove-Item $script -Force
    return ($out -match [regex]::Escape($path))
}

function Get-VolumeFreeBytes([string]$letter) {
    $drive = Get-PSDrive -Name $letter -ErrorAction SilentlyContinue
    if ($drive) { return [int64]$drive.Free }
    return -1
}

switch ($Command) {

    'New' {
        if (-not (Test-Elevated)) {
            Write-Error "New requires elevation (diskpart). Create the VHD once in an admin step; lanes then reuse it."
            exit 3
        }
        $letter = $MountLetter
        if (-not $letter) { $letter = Get-FreeLetter }

        # Clean any leftover attach from a previous run (the file cannot
        # be deleted while a vdisk is attached to it).
        if (Get-AttachState $VhdPath) {
            $script = [System.IO.Path]::GetTempFileName()
            @(
                "select vdisk file=`"$VhdPath`""
                "detach vdisk"
            ) | Set-Content -Path $script -Encoding Ascii
            & diskpart /s $script 2>&1 | Out-String | Out-Null
            Remove-Item $script -Force
            Start-Sleep -Milliseconds 500
        }

        $made = $false
        if (Get-Command New-VHD -ErrorAction SilentlyContinue) {
            try {
                New-VHD -Path $VhdPath -SizeBytes ($SizeMB * 1MB) -Fixed | Out-Null
                $made = $true
            } catch {
                Write-Verbose "New-VHD failed ($($_.Exception.Message)); falling back to diskpart"
            }
        }
        if ($made) {
            # New-VHD made the file; attach + partition + letter via diskpart.
            $script = [System.IO.Path]::GetTempFileName()
            @(
                "select vdisk file=`"$VhdPath`""
                "attach vdisk"
                "create partition primary"
                "format fs=ntfs quick"
                "assign letter=$letter"
            ) | Set-Content -Path $script -Encoding Ascii
            $out = & diskpart /s $script 2>&1 | Out-String
            Remove-Item $script -Force
            if ($LASTEXITCODE -ne 0) { Write-Host $out; exit 4 }
        } else {
            if (Test-Path $VhdPath) { Remove-Item $VhdPath -Force }
            $parent = Split-Path -Parent $VhdPath
            if ($parent -and -not (Test-Path $parent)) {
                New-Item -ItemType Directory -Path $parent -Force | Out-Null
            }
            # ONE diskpart script end-to-end: the volume context from
            # create partition/format is what `assign` needs, so the
            # letter assignment must live in this same script.
            $script = [System.IO.Path]::GetTempFileName()
            @(
                "create vdisk file=`"$VhdPath`" maximum=$SizeMB type=fixed"
                "select vdisk file=`"$VhdPath`""
                "attach vdisk"
                "create partition primary"
                "format fs=ntfs quick"
                "assign letter=$letter"
            ) | Set-Content -Path $script -Encoding Ascii
            $out = & diskpart /s $script 2>&1 | Out-String
            Remove-Item $script -Force
            if ($LASTEXITCODE -ne 0) { Write-Host $out; exit 4 }
        }

        Start-Sleep -Milliseconds 500
        $free = Get-VolumeFreeBytes $letter
        Write-Output ("MOUNTED letter={0} sizeMB={1} freeBytes={2} vhd={3}" -f `
            $letter, $SizeMB, $free, $VhdPath)
        exit 0
    }

    'Fill' {
        if (-not $Path) { Write-Error "Fill requires -Path (a directory on the target volume)"; exit 4 }
        if (-not (Test-Path $Path)) { New-Item -ItemType Directory -Path $Path -Force | Out-Null }
        $block = New-Object byte[] ($BlockSizeMB * 1MB)
        (New-Object Random 1234).NextBytes($block)
        $i = 0
        $total = [int64]0
        try {
            while ($true) {
                $file = Join-Path $Path ("fill_{0:D4}.bin" -f $i)
                $fs = [System.IO.File]::Open($file, 'Create', 'Write')
                try { $fs.Write($block, 0, $block.Length); $fs.Flush() } finally { $fs.Close() }
                $total += $block.Length
                $i++
            }
        } catch [System.IO.IOException] {
            $hr = $_.Exception.HResult
            Write-Output ("ENOSPC hit after {0} files, {1} bytes (HRESULT 0x{2:X8}: {3})" -f `
                $i, $total, $hr, $_.Exception.Message.Split("`n")[0])
            exit 0
        }
    }

    'Status' {
        $attached = Get-AttachState $VhdPath
        Write-Output ("vhd={0} attachedInDiskpart={1}" -f $VhdPath, $attached)
        Get-PSDrive -PSProvider FileSystem | ForEach-Object {
            Write-Output ("  {0}: free={1:N0} used={2:N0}" -f $_.Name, $_.Free, $_.Used)
        }
        exit 0
    }

    'Remove' {
        if (Get-AttachState $VhdPath) {
            $script = [System.IO.Path]::GetTempFileName()
            @(
                "select vdisk file=`"$VhdPath`""
                "detach vdisk"
            ) | Set-Content -Path $script -Encoding Ascii
            $out = & diskpart /s $script 2>&1 | Out-String
            Remove-Item $script -Force
            if ($LASTEXITCODE -ne 0) { Write-Host $out; exit 4 }
        }
        if (Test-Path $VhdPath) { Remove-Item $VhdPath -Force }
        Write-Output "DETACHED+DELETED $VhdPath"
        exit 0
    }
}
