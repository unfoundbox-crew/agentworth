import React from "react";
import { SiteHeader, SiteFooter, InstallLine } from "./SiteChrome";
import { posts, humanDate } from "../content";

export const BlogIndexPage: React.FC = () => (
  <>
    <a className="skip" href="#main">
      Skip to content
    </a>
    <SiteHeader current="blog" />

    <main id="main">
      <div className="wrap">
        <nav className="crumbs" aria-label="Breadcrumb">
          <a href="/">Home</a>
          <span aria-hidden="true">/</span>
          <span aria-current="page">Blog</span>
        </nav>

        <header className="page-head">
          <p className="kicker">Writing</p>
          <h1>Notes from measuring our own agents.</h1>
          <p className="lede">
            Everything here comes from one index on one laptop, and every number
            has a spec or a pull request behind it. Where a measurement is
            narrow, the post says how narrow rather than rounding it into a
            claim.
          </p>
        </header>

        <ol className="post-list">
          {posts.map((p) => (
            <li key={p.slug}>
              <article>
                <p className="post-meta">
                  <time dateTime={p.date}>{humanDate(p.date)}</time>
                  <span aria-hidden="true">&middot;</span>
                  <span>{p.readingMinutes} min read</span>
                </p>
                <h2>
                  <a href={`/blog/${p.slug}/`}>{p.title}</a>
                </h2>
                <p className="post-desc">{p.description}</p>
                {p.tags.length > 0 && (
                  <p className="tags">
                    {p.tags.map((t) => (
                      <span className="tag" key={t}>
                        {t}
                      </span>
                    ))}
                  </p>
                )}
              </article>
            </li>
          ))}
        </ol>

        <section className="close-band">
          <h2>Point it at your own machine.</h2>
          <InstallLine />
        </section>
      </div>
    </main>

    <SiteFooter />
  </>
);
