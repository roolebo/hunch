// hunch's settings in GNOME, as Hunch's Settings window is on a Mac: the
// seat's through hunch-host, the allows in the seat's config, which only
// the human writes.

import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';

import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

import {editConfig, hostName, hunchHost, mic, readConfig} from './hunch.js';

// The dictation keys hunch-host knows on Linux, as the human reads them
const KEYS = [
    ['right-alt', 'Right Alt (AltGr)'],
    ['right-super', 'Right Super'],
    ['right-control', 'Right Ctrl'],
    ['right-shift', 'Right Shift'],
];

const WARM = [
    ['3', '3 minutes'],
    ['10', '10 minutes'],
    ['60', '1 hour'],
    ['always', 'Always'],
];

function combo(title, choices, chosen, choose) {
    const row = new Adw.ComboRow({
        title,
        model: Gtk.StringList.new(choices.map(([, label]) => label)),
        selected: Math.max(0, choices.findIndex(([value]) => value === chosen)),
    });
    row.connect('notify::selected', () => choose(choices[row.selected][0]));
    return row;
}

function toggle(title, active, choose) {
    const row = new Adw.SwitchRow({title, active});
    row.connect('notify::active', () => choose(row.active));
    return row;
}

export default class HunchPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        this._window = window;
        window.set_default_size(560, 640);
        window.add(this._page('Dictation', 'audio-input-microphone-symbolic', this._dictation));
        window.add(this._page('Hosts', 'network-server-symbolic', this._hosts));
        window.add(this._page('Permissions', 'security-medium-symbolic', this._permissions));
        window.add(this._page('About', 'help-about-symbolic', this._about));
    }

    /// A page, filled once hunch-host has answered
    _page(title, icon, fill) {
        const page = new Adw.PreferencesPage({title, icon_name: icon});
        fill.call(this, page).catch(e => this._toast(e.message));
        return page;
    }

    _toast(message) {
        this._window.add_toast(new Adw.Toast({title: message, timeout: 5}));
    }

    /// Runs hunch-host ARGS, saying what went wrong
    _run(...args) {
        hunchHost(args).catch(e => this._toast(e.message));
    }

    async _dictation(page) {
        const seat = JSON.parse(await hunchHost(['seat', 'context']));
        const group = new Adw.PreferencesGroup({
            title: 'Dictation',
            description: 'Hold the key and speak; Esc cancels',
        });
        group.add(combo('Key', KEYS, seat.key, key => this._run('seat', 'key', key)));
        group.add(toggle('Press Enter after the text', seat.submit,
            on => this._run('seat', 'submit', on ? 'on' : 'off')));
        group.add(toggle('Load the model at start', seat.preload,
            on => this._run('seat', 'preload', on ? 'on' : 'off')));
        const warm = combo('Keep the model ready', WARM, seat.warm === null ? 'always' : String(seat.warm),
            minutes => this._run('seat', 'warm', minutes));
        warm.subtitle = 'From the service\'s next start';
        group.add(warm);
        page.add(group);
    }

    /// A group of the hosts in an allow, which the human edits here
    _allow(title, description, get, set) {
        const group = new Adw.PreferencesGroup({title, description});
        const rows = [];
        const fill = () => {
            rows.splice(0).forEach(row => group.remove(row));
            let hosts;
            try {
                hosts = get(readConfig());
            } catch (e) {
                this._toast(e.message);
                return;
            }
            for (const host of hosts) {
                const row = new Adw.ActionRow({title: host});
                const remove = new Gtk.Button({icon_name: 'list-remove-symbolic', valign: Gtk.Align.CENTER});
                remove.add_css_class('flat');
                remove.connect('clicked', () => change(list => list.filter(h => h !== host)));
                row.add_suffix(remove);
                group.add(row);
                rows.push(row);
            }
            const entry = new Adw.EntryRow({title: 'Allow a host, as you ssh to it', show_apply_button: true});
            entry.connect('apply', () => {
                const host = entry.text.trim();
                // ssh would take a host that starts with - for an option
                if (!host || host.startsWith('-') || /\s/.test(host) || host === 'localhost')
                    return this._toast(`${host || 'Nothing'} isn't a host`);
                change(list => list.includes(host) ? list : [...list, host]);
            });
            group.add(entry);
            rows.push(entry);
        };
        const change = edit => {
            try {
                editConfig(config => set(config, edit(get(config))));
            } catch (e) {
                this._toast(e.message);
            }
            fill();
        };
        fill();
        return group;
    }

    async _hosts(page) {
        const microphone = await mic().catch(() => null);
        if (microphone) {
            page.add(this._allow('Speech may go to',
                `Hosts that transcribe what ${microphone.name} hears: ones you ssh to that run hunch-host with a compute`,
                config => config.devices?.[microphone.id]?.allow ?? [],
                (config, hosts) => {
                    config.devices ??= {};
                    config.devices[microphone.id] ??= {};
                    config.devices[microphone.id].allow = hosts;
                }));
        }
        page.add(this._allow('Pages may go to', 'Of the browser, for the agents there',
            config => config.browser?.allow ?? [],
            (config, hosts) => {
                config.browser ??= {};
                config.browser.allow = hosts;
            }));
        page.add(new Adw.PreferencesGroup({
            description: `Data goes to one host at a time, the one you pick in the menu, besides ${hostName('localhost')}; ` +
                'Hunch reaches it by ssh, as you do.',
        }));
    }

    async _permissions(page) {
        const grants = JSON.parse(await hunchHost(['needs', '--all']));
        const group = new Adw.PreferencesGroup({
            title: 'Permissions',
            description: 'What hunch-host needs from you; it never grants itself anything',
        });
        for (const grant of grants) {
            const row = new Adw.ActionRow({
                title: grant.grant,
                subtitle: grant.granted ? `For ${grant.for}`
                    : `For ${grant.for}: usermod -aG input $USER, then log in again`,
            });
            row.add_suffix(new Gtk.Image({
                icon_name: grant.granted ? 'emblem-ok-symbolic' : 'dialog-warning-symbolic',
            }));
            group.add(row);
        }
        page.add(group);
    }

    async _about(page) {
        const credit = (group, name, work, url) => {
            const row = new Adw.ActionRow({title: name, subtitle: work, activatable: true});
            row.connect('activated', () => new Gtk.UriLauncher({uri: url}).launch(this._window, null, null));
            row.add_suffix(new Gtk.Image({icon_name: 'adw-external-link-symbolic'}));
            group.add(row);
        };
        const ideas = new Adw.PreferencesGroup({title: 'Ideas'});
        credit(ideas, 'Vannevar Bush', 'As We May Think, 1945',
            'https://www.theatlantic.com/magazine/archive/1945/07/as-we-may-think/303881/');
        credit(ideas, 'J. C. R. Licklider', 'Man-Computer Symbiosis, 1960',
            'https://groups.csail.mit.edu/medg/people/psz/Licklider.html');
        credit(ideas, 'Douglas Engelbart', 'Augmenting Human Intellect, 1962', 'https://www.dougengelbart.org/');
        page.add(ideas);
        const built = new Adw.PreferencesGroup({title: 'Built on'});
        credit(built, 'ggml', 'Georgi Gerganov and its authors', 'https://github.com/ggml-org/ggml');
        credit(built, 'llama.cpp', 'Its authors', 'https://github.com/ggml-org/llama.cpp');
        credit(built, 'transcribe.cpp', 'Its authors', 'https://github.com/handy-computer/transcribe.cpp');
        page.add(built);
        const inspiration = new Adw.PreferencesGroup({title: 'Inspiration'});
        credit(inspiration, 'Pipit', 'The first dictation app that got Hunch going', 'https://www.pipitvoice.com');
        page.add(inspiration);
    }
}
