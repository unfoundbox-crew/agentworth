import React from "react";
import { SiteHeader, SiteFooter } from "./SiteChrome";
import { humanDate, type Post } from "../content";

export const BlogPostPage: React.FC<{ post: Post }> = ({ post }) => (
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
          <a href="/blog/">Blog</a>
          <span aria-hidden="true">/</span>
          <span aria-current="page">{post.title}</span>
        </nav>

        <article className="post">
          <header className="post-head">
            <p className="post-meta">
              <time dateTime={post.date}>{humanDate(post.date)}</time>
              <span aria-hidden="true">&middot;</span>
              <span>{post.readingMinutes} min read</span>
              <span aria-hidden="true">&middot;</span>
              <span>{post.author}</span>
            </p>
            <h1>{post.title}</h1>
            <p className="lede">{post.description}</p>
            {post.tags.length > 0 && (
              <p className="tags">
                {post.tags.map((t) => (
                  <span className="tag" key={t}>
                    {t}
                  </span>
                ))}
              </p>
            )}
          </header>

          <div
            className="prose post-body"
            dangerouslySetInnerHTML={{ __html: post.html }}
          />
        </article>

        {(post.newer || post.older) && (
          <nav className="post-nav" aria-label="More posts">
            {post.newer ? (
              <a className="pn newer" href={`/blog/${post.newer.slug}/`}>
                <span>Newer</span>
                <b>{post.newer.title}</b>
              </a>
            ) : (
              <span />
            )}
            {post.older && (
              <a className="pn older" href={`/blog/${post.older.slug}/`}>
                <span>Older</span>
                <b>{post.older.title}</b>
              </a>
            )}
          </nav>
        )}
      </div>
    </main>

    <SiteFooter />
  </>
);
