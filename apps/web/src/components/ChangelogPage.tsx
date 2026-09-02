import React from "react";
import { SiteHeader, SiteFooter, InstallLine, REPO } from "./SiteChrome";
import { releases, downloads, humanDate, type Release } from "../content";

/**
 * Downloads, not users. Both numbers are fetched once at build time and
 * printed with the window they cover, because a bare count with no window is
 * not a measurement. npm's counter lags its own publish by a day or two, so
 * the window end is usually behind today — say so rather than round it away.
 */
const Downloads: React.FC = () => {
  const { npm, github, fetchedAt } = downloads;
  if (!npm && !github) return null;

  return (
    <dl className="dl-stats">
      {github && (
        <div>
          <dt>Release binaries</dt>
          <dd>
            <b>{github.assets.toLocaleString()}</b> downloads
            <span>across {github.releases} releases</span>
          </dd>
        </div>
      )}
      {npm && (
        <div>
          <dt>npm, last 30 days</dt>
          <dd>
            <b>{npm.downloads.toLocaleString()}</b> downloads
            <span>
              {npm.start} to {npm.end}
            </span>
          </dd>
        </div>
      )}
      <div>
        <dt>Counted</dt>
        <dd>
          <b>{fetchedAt}</b>
          <span>at build time, not on your visit</span>
        </dd>
      </div>
    </dl>
  );
};

const ReleaseBody: React.FC<{ release: Release }> = ({ release }) => (
  <>
    {release.sections.map((s) => (
      <section className="rel-section" key={s.title}>
        <h3>{s.title}</h3>
        <div className="prose" dangerouslySetInnerHTML={{ __html: s.html }} />
      </section>
    ))}
  </>
);

export const ChangelogPage: React.FC = () => {
  const [latest, ...older] = releases;

  return (
    <>
      <a className="skip" href="#main">
        Skip to content
      </a>
      <SiteHeader current="changelog" />

      <main id="main">
        <div className="wrap">
          <nav className="crumbs" aria-label="Breadcrumb">
            <a href="/">Home</a>
            <span aria-hidden="true">/</span>
            <span aria-current="page">Changelog</span>
          </nav>

          <header className="page-head">
            <p className="kicker">Releases</p>
            <h1>What changed, and which pull request changed it.</h1>
            <p className="lede">
              Every line below is a real change with its PR number beside it.
              The format is{" "}
              <a href="https://keepachangelog.com/en/1.1.0/">
                Keep a Changelog
              </a>
              , generated at build time from{" "}
              <a href={`${REPO}/blob/main/CHANGELOG.md`}>CHANGELOG.md</a> in the
              repository &mdash; so this page cannot drift from the file the
              releases are cut from.
            </p>
            <Downloads />
          </header>

          <div className="rel-layout">
            <article className="rel-main">
              <article className="rel rel-latest" id={latest.id}>
                <div className="rel-head">
                  <h2>
                    <a href={`#${latest.id}`} className="ver">
                      {latest.version}
                    </a>
                    <span className="tag-latest">Latest</span>
                  </h2>
                  <p className="rel-meta">
                    <time dateTime={latest.date}>{humanDate(latest.date)}</time>
                    <span aria-hidden="true">&middot;</span>
                    <span>{latest.changeCount} changes</span>
                    <span aria-hidden="true">&middot;</span>
                    <a href={`${REPO}/releases/tag/v${latest.version}`}>
                      Release notes and binaries
                    </a>
                  </p>
                </div>
                <ReleaseBody release={latest} />
              </article>

              {older.map((r) => (
                <article className="rel" id={r.id} key={r.version}>
                  <div className="rel-head">
                    <h2>
                      <a href={`#${r.id}`} className="ver">
                        {r.version}
                      </a>
                    </h2>
                    <p className="rel-meta">
                      <time dateTime={r.date}>{humanDate(r.date)}</time>
                      <span aria-hidden="true">&middot;</span>
                      <span>{r.changeCount} changes</span>
                      <span aria-hidden="true">&middot;</span>
                      <a href={`${REPO}/releases/tag/v${r.version}`}>Tag</a>
                    </p>
                  </div>
                  <ReleaseBody release={r} />
                </article>
              ))}
            </article>

            <nav className="rel-rail" aria-label="Releases">
              <p className="rail-head">All releases</p>
              <ol>
                {releases.map((r) => (
                  <li key={r.version}>
                    <a href={`#${r.id}`}>
                      <span className="rail-v">{r.version}</span>
                      <span className="rail-d">{r.date}</span>
                    </a>
                  </li>
                ))}
              </ol>
              <p className="rail-foot">
                <a href="/changelog/rss.xml">Subscribe by RSS</a>
              </p>
            </nav>
          </div>

          <section className="close-band">
            <h2>Point it at your own machine.</h2>
            <InstallLine />
          </section>
        </div>
      </main>

      <SiteFooter />
    </>
  );
};
