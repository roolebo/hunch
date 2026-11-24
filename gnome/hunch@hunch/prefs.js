// hunch's settings in GNOME, as Hunch's Settings window is on a Mac: the
// seat's through hunch-seat, the servers in the seat's config, which only
// the human writes.

import Adw from 'gi://Adw';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk';

import {ExtensionPreferences} from
  'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

import {
  mount, mounted, readSeat, readTree, seatCtl, takePage, writeTree,
} from './hunch.js';

// The dictation keys hunch-seat knows on Linux, as the human reads them
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
    // Wide enough that the pages' switcher stays in the header bar
    window.set_default_size(720, 480);
    window.add(this._page('dictation', 'Dictation',
      'audio-input-microphone-symbolic', this._dictation));
    window.add(this._page('inference', 'Inference', 'network-server-symbolic',
      this._inference));
    window.add(this._page('permissions', 'Permissions',
      'security-medium-symbolic', this._permissions));
    window.add(this._page('about', 'About', 'help-about-symbolic',
      this._about));
    const page = takePage();
    if (page)
      window.visible_page_name = page;
  }

  /// A page, filled once hunch-seat has answered
  _page(name, title, icon, fill) {
    const page = new Adw.PreferencesPage({name, title, icon_name: icon});
    fill.call(this, page).catch(e => this._toast(e.message));
    return page;
  }

  _toast(message) {
    this._window.add_toast(new Adw.Toast({title: message, timeout: 5}));
  }

  /// Writes LINE to the seat's ctl, saying what went wrong
  _ctl(line) {
    seatCtl(line).catch(e => this._toast(e.message));
  }

  async _dictation(page) {
    const seat = JSON.parse(await readSeat('context'));
    // Where speech goes and its model, again each time either changes or
    // the servers do
    let speech = null;
    this._refreshSpeech = async () => {
      const group = await this._speech();
      if (speech)
        page.remove(speech);
      speech = group;
      page.add(speech);
      // Above the rest
      page.remove(rest);
      page.add(rest);
    };
    const rest = new Adw.PreferencesGroup({
      description: 'Hold the key and speak; Esc cancels',
    });
    rest.add(combo('Key', KEYS, seat.key,
      key => this._ctl(`key ${key}`)));
    rest.add(toggle('Press Enter after the text', seat.submit,
      on => this._ctl(`submit ${on ? 'on' : 'off'}`)));
    rest.add(toggle('Load the model at start', seat.preload,
      on => this._ctl(`preload ${on ? 'on' : 'off'}`)));
    const kept = seat.warm === null ? 'always' : String(seat.warm);
    const warm = combo('Keep the model ready', WARM, kept,
      minutes => this._ctl(`warm ${minutes}`));
    warm.subtitle = 'From the service\'s next start';
    rest.add(warm);
    page.add(rest);
    await this._refreshSpeech();
  }

  /// The model speech becomes text with: one of inference/ that takes
  /// audio and answers text
  async _speech() {
    const seat = JSON.parse(await readSeat('context'));
    // A server's come over the network
    const models = await readTree('inference/models')
      .then(JSON.parse, () => []);
    const group = new Adw.PreferencesGroup({title: 'Dictation'});
    const transcribes = models.filter(m =>
      (m.inputs ?? []).some(i => i.startsWith('audio/')) &&
      (m.outputs ?? []).some(o => o.startsWith('text/')));
    // None until one is chosen, so choosing the first one is a change
    const ids = transcribes.map(m => [m.id, m.name ?? m.id]);
    if (!transcribes.some(m => m.id === seat.model))
      ids.unshift(['', 'None']);
    const model = combo('Model', ids, seat.model ?? '',
      id => id && this._ctl(`model ${id}`));
    if (transcribes.length === 0)
      model.subtitle = 'No model of Inference says it takes speech';
    group.add(model);
    return group;
  }

  /// Where speech goes: This Computer, or a server by its URL, with the
  /// key this computer logs in with, which hunch-host keeps in the Secret
  /// Service; the key's field shows none
  async _inference(page) {
    const group = new Adw.PreferencesGroup({
      title: 'Inference',
      description: 'This computer runs its own models; a server is an ' +
        'OpenAI-compatible one',
    });
    const now = mounted();
    const local = now === 'local:';
    const server = new Adw.EntryRow({
      title: 'Server, e.g. http://thinkstation:8080/v1',
      text: local ? '' : now, show_apply_button: true,
      visible: !local,
    });
    const key = new Adw.PasswordEntryRow({
      title: 'Key, if the server asks for one', show_apply_button: true,
      visible: !local,
    });
    const where = combo('Transcribe on',
      [['local:', 'This Computer'], ['server', 'A Server']],
      local ? 'local:' : 'server', choice => {
        server.visible = key.visible = choice === 'server';
        if (choice === 'local:')
          this._mount('local:');
      });
    server.connect('apply', () => {
      const url = server.text.trim();
      let uri = null;
      try {
        uri = GLib.Uri.parse(url, GLib.UriFlags.NONE);
      } catch {
        // Not a URL, as below
      }
      if (!['http', 'https'].includes(uri?.get_scheme()) || !uri.get_host())
        return this._toast(`${url || 'Nothing'} isn't a server's URL`);
      this._mount(url);
    });
    key.connect('apply', () => {
      writeTree('inference/login', `${key.text}\n`)
        .then(() => this._toast('Kept the key'))
        .catch(e => this._toast(e.message));
      key.text = '';
    });
    [where, server, key].forEach(row => group.add(row));
    page.add(group);
  }

  /// Mounts ENDPOINT on inference/, then shows its models
  _mount(endpoint) {
    try {
      mount(endpoint);
    } catch (e) {
      this._toast(e.message);
    }
    this._refreshSpeech?.().catch(e => this._toast(e.message));
  }

  async _permissions(page) {
    const grants = JSON.parse(await readSeat('needs'));
    const group = new Adw.PreferencesGroup({
      title: 'Permissions',
      description:
        'What hunch-seat needs from you; it never grants itself anything',
    });
    for (const grant of grants) {
      const row = new Adw.ActionRow({
        title: grant.grant,
        subtitle: grant.granted ? `For ${grant.for}`
          : `For ${grant.for}: usermod -aG input $USER, then log in again`,
      });
      row.add_suffix(new Gtk.Image({
        icon_name: grant.granted
          ? 'emblem-ok-symbolic' : 'dialog-warning-symbolic',
      }));
      group.add(row);
    }
    page.add(group);
  }

  async _about(page) {
    const credit = (group, name, work, url) => {
      const row = new Adw.ActionRow({
        title: name, subtitle: work, activatable: true,
      });
      row.connect('activated', () =>
        new Gtk.UriLauncher({uri: url}).launch(this._window, null, null));
      row.add_suffix(new Gtk.Image({icon_name: 'adw-external-link-symbolic'}));
      group.add(row);
    };
    const ideas = new Adw.PreferencesGroup({
      title: 'Ideas',
      description: 'Hunch stays at the periphery of your attention, and ' +
        'comes to the center when you call it',
    });
    credit(ideas, "hunch's README", 'The papers hunch builds on',
      'https://github.com/roolebo/hunch#references');
    page.add(ideas);
    const built = new Adw.PreferencesGroup({title: 'Built on'});
    credit(built, 'ggml', 'Georgi Gerganov and its authors',
      'https://github.com/ggml-org/ggml');
    credit(built, 'llama.cpp', 'Its authors',
      'https://github.com/ggml-org/llama.cpp');
    credit(built, 'transcribe.cpp', 'Its authors',
      'https://github.com/handy-computer/transcribe.cpp');
    page.add(built);
    const inspiration = new Adw.PreferencesGroup({title: 'Inspiration'});
    credit(inspiration, 'Pipit', 'The first dictation app that got Hunch going',
      'https://www.pipitvoice.com');
    page.add(inspiration);
  }
}
