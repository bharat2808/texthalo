import inventory from './third-party-licenses.json';
import { useWebsiteI18n } from './i18n';

const additional = [
  { name: 'DM Sans', license: 'OFL-1.1', scope: 'fonts', url: 'https://github.com/google/fonts/tree/main/ofl/dmsans', notices: ['DM-Sans-OFL.txt'] },
  { name: 'DM Mono', license: 'OFL-1.1', scope: 'fonts', url: 'https://github.com/google/fonts/tree/main/ofl/dmmono', notices: ['DM-Mono-OFL.txt'] },
  { name: 'Manrope', license: 'OFL-1.1', scope: 'fonts', url: 'https://github.com/google/fonts/tree/main/ofl/manrope', notices: ['Manrope-OFL.txt'] },
  { name: 'ONNX Runtime 1.28.0', license: 'MIT', scope: 'bundled', url: 'https://github.com/microsoft/onnxruntime/tree/v1.28.0', notices: ['onnxruntime-MIT.txt', 'onnxruntime-NOTICES.txt'] },
  { name: 'Misaki', license: 'Apache-2.0', scope: 'pronunciation', url: 'https://github.com/hexgrad/misaki/tree/fba1236595f2d2bf21d414ba6e57d25256afada3', notices: ['misaki-Apache-2.0.txt', 'CJK-NOTICES.txt'] },
  { name: 'Cutlet', license: 'MIT', scope: 'pronunciation', url: 'https://github.com/polm/cutlet', notices: ['cutlet-MIT.txt'] },
  { name: 'pinyin-to-ipa', license: 'MIT', scope: 'pronunciation', url: 'https://github.com/stefantaubert/pinyin-to-ipa', notices: ['pinyin-to-ipa-MIT.txt'] },
  { name: 'NAIST-JDIC', license: 'BSD-3-Clause', scope: 'pronunciation', url: 'https://github.com/jpreprocess/naist-jdic/tree/v0.1.3', notices: ['naist-jdic-BSD.txt'] },
  { name: 'pinyin-pro 3.29.4', license: 'MIT', scope: 'pronunciation', url: 'https://github.com/zh-lx/pinyin-pro/tree/v3.29.4', notices: ['pinyin-pro-MIT.txt'] },
  { name: 'Jieba', license: 'MIT', scope: 'pronunciation', url: 'https://github.com/fxsjy/jieba', notices: ['jieba-MIT.txt'] },
  { name: 'Kokoro-82M v1.0 (ONNX)', license: 'Apache-2.0', scope: 'optional', url: 'https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/tree/1939ad2a8e416c0acfeecc08a694d14ef25f2231', notices: ['kokoro-Apache-2.0.txt'] },
  { name: 'Chatterbox Multilingual (ONNX)', license: 'MIT', scope: 'optional', url: 'https://huggingface.co/onnx-community/chatterbox-multilingual-ONNX/tree/452d3f434aa592098f1eedac9099f33642ab2da5', notices: ['chatterbox-MIT.txt'] },
  { name: 'Whisper', license: 'MIT', scope: 'optional', url: 'https://huggingface.co/ggerganov/whisper.cpp/tree/5359861c739e955e79d9a303bcbc70fb988958b1', notices: ['whisper-MIT.txt'] },
  { name: 'eSpeak NG 1.52.0', license: 'GPL-3.0-or-later', scope: 'separate', url: 'https://github.com/espeak-ng/espeak-ng/tree/1.52.0', notices: ['espeak-ng-COPYING.txt'] },
  { name: 'pcaudiolib 1.3', license: 'GPL-3.0-or-later', scope: 'separate', url: 'https://github.com/espeak-ng/pcaudiolib/tree/1.3', notices: ['pcaudiolib-COPYING.txt'] },
];

export function ThirdPartyLicenses() {
  const i18n = useWebsiteI18n();
  const t = i18n.t;
  const scopes: Record<string, string> = {
    fonts: t('Website typography'), bundled: t('Bundled runtime'), pronunciation: t('Pronunciation and dictionaries'),
    optional: t('Optional model download'), separate: t('Separately installed language support'),
  };
  return <article className="third-party-licenses">
    <header className="article-header"><h1>{t('Third-party licenses')}</h1>
      <p className="article-deck">{t('Licenses and attribution notices for the libraries, models, and language tools used by TextHalo.')}</p>
      <p>{t('These notices apply to third-party components only. Each component retains its original license.')}</p>
      <p>{t('Version {0}', [inventory.version])} · <a href="/licenses/dependencies.json" download>{t('Download dependency inventory')}</a></p>
    </header>
    <div className="article-body">
      <h2>{t('Speech engines, models, dictionaries, and fonts')}</h2>
      <p>{t('Voice models are downloaded when you choose them. eSpeak NG and pcaudiolib are installed separately from the app bundle and retain their GPL terms.')}</p>
      <div className="license-table-wrap"><table className="license-table"><thead><tr><th>{t('Component')}</th><th>{t('License')}</th><th>{t('Used for')}</th><th>{t('Notices')}</th></tr></thead><tbody>{additional.map(entry => <tr key={entry.name}>
        <td><a href={entry.url}>{entry.name}</a></td><td>{entry.license}</td><td>{scopes[entry.scope]}</td>
        <td>{entry.notices.map(name => <a key={name} href={`/licenses/attributions/${name}`}>{name}</a>)}{!entry.notices.length && <a href={entry.url}>{t('Upstream license')}</a>}</td>
      </tr>)}</tbody></table></div>
      <h2>{t('Dependency inventory')}</h2>
      <p>{t('This inventory includes direct and transitive dependencies from the locked Rust and npm manifests. Build tools and platform-specific packages are listed separately from runtime libraries.')}</p>
      <p>{t('Original license and notice files are provided where included in the dependency package. Upstream links provide source code and additional licensing details.')}</p>
      {(['runtime', 'build'] as const).map(scope => <details key={scope} open={scope === 'runtime'}>
        <summary>{t(scope === 'runtime' ? 'Runtime libraries' : 'Build tools and other locked packages')} ({inventory.entries.filter(entry => entry.scope === scope).length})</summary>
        <div className="license-table-wrap"><table className="license-table"><thead><tr><th>{t('Component')}</th><th>{t('Version')}</th><th>{t('License')}</th><th>{t('Notices')}</th></tr></thead><tbody>{inventory.entries.filter(entry => entry.scope === scope).map(entry => <tr key={`${entry.ecosystem}:${entry.name}:${entry.version}`}>
          <td><a href={entry.url}>{entry.name}</a><small>{entry.ecosystem}</small></td><td>{entry.version}</td><td>{entry.license}</td>
          <td>{entry.notices.map(notice => <a key={notice.href} href={notice.href}>{notice.name}</a>)}{!entry.notices.length && <a href={entry.url}>{t('Upstream license')}</a>}</td>
        </tr>)}</tbody></table></div>
      </details>)}
    </div>
  </article>;
}
