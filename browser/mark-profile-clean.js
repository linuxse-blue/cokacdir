const fs = require("fs");

const preferencesPath = "/home/kernel/user-data/Default/Preferences";
const temporaryPath = `${preferencesPath}.clean`;
const preferences = JSON.parse(fs.readFileSync(preferencesPath, "utf8"));

preferences.profile ||= {};
preferences.profile.exit_type = "Normal";
preferences.profile.exited_cleanly = true;

fs.writeFileSync(temporaryPath, JSON.stringify(preferences));
fs.renameSync(temporaryPath, preferencesPath);
