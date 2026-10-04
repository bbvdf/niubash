<#
.SYNOPSIS
  file-locker.ps1 -- holds an exclusive lock on a file to simulate an
  antivirus scanner or a crashed editor holding ~/.niubashrc open
  (adversarial-matrix lane AM06).

.DESCRIPTION
  Opens a FileStream with FileShare::None so ANY other writer (or reader-
  with-write) gets a sharing violation (IOException 0x80070020) for the
  hold duration, then exits and releases. This is the "locked files
  (AV-style)" injection: `niu plugin enable/disable` and the rc writer
  must fail LOUDLY (nonzero + named reason + rc intact), never print
  success while the write was dropped -- the niu#175 "fake success" class.

  PowerShell 5.1 compatible. Stdlib only.

.USAGE
  # hold the lock for 30s (blocking; run from another process/job)
  powershell -File scripts/adversarial/file-locker.ps1 -Path <file> -Seconds 30

  # from the probe: is it locked right now? (exit 0 = lockable, 9 = locked)
  powershell -File scripts/adversarial/file-locker.ps1 -Test <file>

.EXIT CODES
  Test mode: 0 = the file can be opened for write (not locked);
             9 = sharing violation (locked); 2 = missing/other error.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Path,
    [int]$Seconds = 10,
    [switch]$Test
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $Path)) {
    Write-Error "file not found: $Path"
    exit 2
}

$full = (Resolve-Path $Path).Path

if ($Test) {
    try {
        $fs = [System.IO.File]::Open($full, 'Open', 'ReadWrite', 'None')
        $fs.Close()
        Write-Output "UNLOCKED $full"
        exit 0
    } catch [System.IO.IOException] {
        Write-Output ("LOCKED {0} ({1})" -f $full, $_.Exception.Message.Split("`n")[0])
        exit 9
    } catch {
        Write-Output ("ERROR {0} ({1})" -f $full, $_.Exception.Message.Split("`n")[0])
        exit 2
    }
}

Write-Output "LOCKING $full for ${Seconds}s (FileShare::None -- AV shape)"
$fs = [System.IO.File]::Open($full, 'Open', 'ReadWrite', 'None')
try {
    Start-Sleep -Seconds $Seconds
} finally {
    $fs.Close()
    Write-Output "RELEASED $full"
}
exit 0
