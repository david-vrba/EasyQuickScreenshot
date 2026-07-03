# End-to-end test: simulates the hotkey + a rectangle drag against a RUNNING eqs.exe.
# Moves the real mouse for ~1 second, then the output file should exist — assert on it yourself.
# Usage: pwsh scripts/e2e-test.ps1            # quick mode (Ctrl+Alt+Q)
#        pwsh scripts/e2e-test.ps1 -Key E     # save mode  (Ctrl+Alt+E)

param(
    [ValidateSet("Q", "E")] [string]$Key = "Q",
    [int]$FromX = 300, [int]$FromY = 300,
    [int]$ToX = 700, [int]$ToY = 550,
    # Time for the app to capture + show the overlay before dragging.
    # Too short and the drag lands on the desktop instead of the overlay.
    [int]$SettleMs = 1800
)

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class EqsInput {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    public const uint KEYUP = 0x0002;
    public const uint LEFTDOWN = 0x0002;
    public const uint LEFTUP = 0x0004;
}
'@

$VK_CONTROL = 0x11; $VK_MENU = 0x12
$vkKey = [byte][char]$Key

[EqsInput]::keybd_event($VK_CONTROL, 0, 0, [UIntPtr]::Zero)
[EqsInput]::keybd_event($VK_MENU, 0, 0, [UIntPtr]::Zero)
[EqsInput]::keybd_event($vkKey, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 80
[EqsInput]::keybd_event($vkKey, 0, [EqsInput]::KEYUP, [UIntPtr]::Zero)
[EqsInput]::keybd_event($VK_MENU, 0, [EqsInput]::KEYUP, [UIntPtr]::Zero)
[EqsInput]::keybd_event($VK_CONTROL, 0, [EqsInput]::KEYUP, [UIntPtr]::Zero)

# Give the app time to capture the screen and show the overlay
Start-Sleep -Milliseconds $SettleMs

[EqsInput]::SetCursorPos($FromX, $FromY) | Out-Null
Start-Sleep -Milliseconds 120
[EqsInput]::mouse_event([EqsInput]::LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 100
foreach ($step in 1..8) {
    $x = $FromX + [int](($ToX - $FromX) * $step / 8)
    $y = $FromY + [int](($ToY - $FromY) * $step / 8)
    [EqsInput]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 30
}
Start-Sleep -Milliseconds 100
[EqsInput]::mouse_event([EqsInput]::LEFTUP, 0, 0, 0, [UIntPtr]::Zero)

Start-Sleep -Milliseconds 800
Write-Output "done — check the shots folder"
