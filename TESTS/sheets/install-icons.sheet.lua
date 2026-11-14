-- The hand check after the icons of the family changed: install the hub again and look at it where Windows shows it.
-- A Claude session may not install (it sees a redirected AppData and HKCU), so a person does it; the sheet asks.
-- Whether the installed program carries the current icon needs no eye: the second question runs the check of the
-- icon generator (tasks-vault TOOLS/scripts/app-icons, icons.mjs exe), which compares the images byte for byte.
--   testing sheets step install-icons
return {
	id = "hub-desktop.install-icons",
	feature = "hub-desktop.install",
	title = "hub-desktop: installed again, the program, its links and its window show the current icon",
	task = "ALL/handlauf-hub-rules-und-docmap-neu-installieren-damit-program",
	engine = "manual",
	steps = {
		{
			ask = "In a normal terminal (not a Claude session), scripts\\build-install.ps1 -InstallArgs --shortcut"
				.. " built and installed the hub?",
		},
		{
			ask = "node $REPOS_DIR/tasks-vault/TOOLS/scripts/app-icons/icons.mjs exe"
				.. " $env:LOCALAPPDATA\\hub-desktop\\bin\\hub-desktop.exe hub-desktop says: carries the current icon?",
		},
		{ ask = "The Start menu has a link Hub, and it shows the window with the spokes of the hub?" },
		{
			ask = "The desktop link Hub shows the same icon, taken from the program"
				.. " (Properties, Change Icon names hub-desktop.exe, no file below the repos)?",
		},
		{ ask = "The window of the hub and its taskbar button show the new icon?" },
		{
			ask = "Explorer shows the new icon on the program and the links"
				.. " (an old picture that sticks: ie4uinit -show, then look again)?",
		},
	},
}
