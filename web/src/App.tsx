import { useCallback, useEffect, useMemo, useState } from 'react'

const publishedBaseUrl = (
  (import.meta.env.DEV ? '/content' : import.meta.env.VITE_PUBLISHED_CONTENT_URL) ??
  'https://d81ul6xa7pt91.cloudfront.net'
).replace(/\/$/, '')
const adminBaseUrl = (
  import.meta.env.VITE_ADMIN_API_URL ??
  'https://hzu0shchx5.execute-api.eu-west-2.amazonaws.com'
).replace(/\/$/, '')

type FolderNode = {
  type: 'folder'
  id: string
  parentId: string | null
  title: string
  position: number
}

type NoteNode = {
  type: 'note'
  id: string
  parentId: string | null
  title: string
  slug: string
  position: number
  status: 'draft' | 'published'
  publishedRevision?: string
}

type TreeManifest = {
  nodes: Array<FolderNode | NoteNode>
}

type DocumentNode = {
  type?: string
  text?: string
  attrs?: { level?: number }
  content?: DocumentNode[]
}

type PublishedNote = {
  document: DocumentNode
}

type PublishedNoteResponse = {
  revision: string
  publicPath: string
}

function textContent(node: DocumentNode): string {
  if (node.type === 'text') return node.text ?? ''
  return node.content?.map(textContent).join('') ?? ''
}

function documentToText(document: DocumentNode): string {
  return document.content?.map(textContent).join('\n\n') ?? ''
}

function textToDocument(text: string): DocumentNode {
  return {
    type: 'doc',
    content: text.split(/\n{2,}/).map((paragraph) => ({
      type: 'paragraph',
      content: paragraph ? [{ type: 'text', text: paragraph }] : [],
    })),
  }
}

function renderBlock(node: DocumentNode, key: number): React.ReactNode {
  const content = node.content?.map((child, index) => renderBlock(child, index))

  switch (node.type) {
    case 'text':
      return node.text
    case 'heading': {
      const level = Math.min(Math.max(node.attrs?.level ?? 2, 1), 3)
      const Heading = `h${level}` as 'h1' | 'h2' | 'h3'
      return <Heading key={key}>{content}</Heading>
    }
    case 'paragraph':
      return <p key={key}>{content}</p>
    case 'bulletList':
    case 'bullet_list':
      return <ul key={key}>{content}</ul>
    case 'orderedList':
    case 'ordered_list':
      return <ol key={key}>{content}</ol>
    case 'listItem':
    case 'list_item':
      return <li key={key}>{content}</li>
    case 'blockquote':
      return <blockquote key={key}>{content}</blockquote>
    case 'codeBlock':
    case 'code_block':
      return <pre key={key}>{textContent(node)}</pre>
    case 'horizontalRule':
    case 'horizontal_rule':
      return <hr key={key} />
    default:
      return <div key={key}>{content}</div>
  }
}

function noteSlugFromHash() {
  return decodeURIComponent(window.location.hash.slice(1))
}

function App() {
  const [publicTree, setPublicTree] = useState<TreeManifest | null>(null)
  const [publicTreeError, setPublicTreeError] = useState<string | null>(null)
  const [privateTree, setPrivateTree] = useState<TreeManifest | null>(null)
  const [privateTreeError, setPrivateTreeError] = useState<string | null>(null)
  const [selectedSlug, setSelectedSlug] = useState(noteSlugFromHash)
  const [publishedNote, setPublishedNote] = useState<PublishedNote | null>(null)
  const [publishedNoteError, setPublishedNoteError] = useState<string | null>(null)
  const [draftNote, setDraftNote] = useState<PublishedNote | null>(null)
  const [draftEtag, setDraftEtag] = useState<string | null>(null)
  const [editorText, setEditorText] = useState('')
  const [adminMessage, setAdminMessage] = useState('')
  const [adminOpen, setAdminOpen] = useState(false)
  const [adminView, setAdminView] = useState<'draft' | 'published'>('draft')
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const [saving, setSaving] = useState(false)
  const [publishing, setPublishing] = useState(false)
  const [creating, setCreating] = useState(false)
  const [createOpen, setCreateOpen] = useState(false)
  const [title, setTitle] = useState('')
  const [slug, setSlug] = useState('')

  const loadPublicTree = useCallback(async () => {
    setPublicTreeError(null)
    try {
      const response = await fetch(`${publishedBaseUrl}/tree.json`, {
        // The manifest chooses the current immutable note revision. Never let a
        // browser reuse an older manifest after a publish.
        cache: 'no-store',
      })
      // A private S3 origin responds with 403 for a missing tree.json. Until
      // the first publish, that means the public site is simply empty.
      if (response.status === 403 || response.status === 404) {
        const emptyTree = { nodes: [] }
        setPublicTree(emptyTree)
        return emptyTree
      }
      if (!response.ok) throw new Error(`The public tree is unavailable (${response.status}).`)
      const nextTree = await response.json() as TreeManifest
      setPublicTree(nextTree)
      return nextTree
    } catch (error) {
      setPublicTreeError(error instanceof Error ? error.message : 'Unable to load published notes.')
      return null
    }
  }, [])

  const loadPrivateTree = useCallback(async () => {
    setPrivateTreeError(null)
    try {
      const response = await fetch(`${adminBaseUrl}/admin/tree`)
      if (!response.ok) throw new Error(`Admin API unavailable (${response.status}).`)
      const nextTree = await response.json() as TreeManifest
      setPrivateTree(nextTree)
      return nextTree
    } catch (error) {
      setPrivateTreeError(error instanceof Error ? error.message : 'Unable to load private notes.')
      return null
    }
  }, [])

  useEffect(() => {
    void loadPublicTree()
  }, [loadPublicTree])

  useEffect(() => {
    if (adminOpen && !privateTree) void loadPrivateTree()
  }, [adminOpen, loadPrivateTree, privateTree])

  useEffect(() => {
    const onHashChange = () => setSelectedSlug(noteSlugFromHash())
    window.addEventListener('hashchange', onHashChange)
    return () => window.removeEventListener('hashchange', onHashChange)
  }, [])

  const publicNotes = useMemo(
    () =>
      (publicTree?.nodes.filter((node): node is NoteNode => node.type === 'note' && node.status === 'published' && Boolean(node.publishedRevision)) ?? []).sort(
        (left, right) => left.position - right.position,
      ),
    [publicTree],
  )
  const privateNotes = useMemo(
    () => (privateTree?.nodes.filter((node): node is NoteNode => node.type === 'note') ?? []).sort(
      (left, right) => left.position - right.position,
    ),
    [privateTree],
  )
  const notes = adminOpen ? privateNotes : publicNotes
  const selectedNote = notes.find((candidate) => candidate.slug === selectedSlug) ?? notes[0]
  const selectedNoteId = selectedNote?.id
  const selectedPublishedRevision = selectedNote?.publishedRevision
  const showPublished = !adminOpen || adminView === 'published'

  useEffect(() => {
    if (!selectedNoteId || !selectedPublishedRevision || !showPublished) return
    const controller = new AbortController()
    setPublishedNote(null)
    setPublishedNoteError(null)
    fetch(
      `${publishedBaseUrl}/notes/${selectedNoteId}/${selectedPublishedRevision}.json`,
      { signal: controller.signal },
    )
      .then(async (response) => {
        if (!response.ok) throw new Error(`This note is unavailable (${response.status}).`)
        return response.json() as Promise<PublishedNote>
      })
      .then(setPublishedNote)
      .catch((error: unknown) => {
        if (!(error instanceof DOMException && error.name === 'AbortError')) {
          setPublishedNoteError(error instanceof Error ? error.message : 'Unable to load this note.')
        }
      })
    return () => controller.abort()
  }, [selectedNoteId, selectedPublishedRevision, showPublished])

  useEffect(() => {
    if (!adminOpen || adminView !== 'draft' || !selectedNoteId) return
    const controller = new AbortController()
    setDraftNote(null)
    setDraftEtag(null)
    setAdminMessage('Loading draft…')
    fetch(`${adminBaseUrl}/admin/notes/${selectedNoteId}`, { signal: controller.signal })
      .then(async (response) => {
        if (!response.ok) throw new Error(`Draft unavailable (${response.status}).`)
        return { draft: await response.json() as PublishedNote, etag: response.headers.get('etag') }
      })
      .then(({ draft, etag }) => {
        setDraftNote(draft)
        setDraftEtag(etag)
        setEditorText(documentToText(draft.document))
        setAdminMessage('')
      })
      .catch((error: unknown) => {
        if (!(error instanceof DOMException && error.name === 'AbortError')) {
          setAdminMessage(error instanceof Error ? error.message : 'Unable to load draft.')
        }
      })
    return () => controller.abort()
  }, [adminOpen, adminView, selectedNoteId])

  const selectNote = (selected: NoteNode) => {
    setPublishedNote(null)
    setPublishedNoteError(null)
    setSelectedSlug(selected.slug)
    window.location.hash = encodeURIComponent(selected.slug)
    if (adminOpen) setAdminView('draft')
  }

  const saveDraft = async () => {
    if (!selectedNoteId || !draftEtag) return
    setSaving(true)
    setAdminMessage('Saving…')
    try {
      const response = await fetch(`${adminBaseUrl}/admin/notes/${selectedNoteId}/draft`, {
        method: 'PUT',
        body: JSON.stringify({ document: textToDocument(editorText), etag: draftEtag }),
      })
      if (response.status === 409) throw new Error('This draft changed elsewhere. Reload it before saving.')
      if (!response.ok) throw new Error(`Saving failed (${response.status}).`)
      const updated = await response.json() as PublishedNote
      setDraftNote(updated)
      setDraftEtag(response.headers.get('etag'))
      setAdminMessage('Draft saved. Publish it when you are ready.')
    } catch (error) {
      setAdminMessage(error instanceof Error ? error.message : 'Saving failed.')
    } finally {
      setSaving(false)
    }
  }

  const publish = async () => {
    if (!selectedNoteId) return
    setPublishing(true)
    setAdminMessage('Publishing…')
    try {
      const response = await fetch(`${adminBaseUrl}/admin/notes/${selectedNoteId}/publish`, { method: 'POST' })
      if (!response.ok) throw new Error(`Publishing failed (${response.status}).`)
      const result = await response.json() as PublishedNoteResponse
      setPrivateTree((current) => current && {
        ...current,
        nodes: current.nodes.map((node) => node.type === 'note' && node.id === selectedNoteId
          ? { ...node, status: 'published', publishedRevision: result.revision }
          : node),
      })
      await loadPublicTree()
      setAdminMessage(`Published at ${result.publicPath}`)
    } catch (error) {
      setAdminMessage(error instanceof Error ? error.message : 'Publishing failed.')
    } finally {
      setPublishing(false)
    }
  }

  const createNote = async (event: React.FormEvent) => {
    event.preventDefault()
    setCreating(true)
    setAdminMessage('Creating note…')
    try {
      const response = await fetch(`${adminBaseUrl}/admin/notes`, {
        method: 'POST',
        body: JSON.stringify({ title, slug }),
      })
      if (!response.ok) throw new Error(`Creating the note failed (${response.status}).`)
      const created = await response.json() as { id: string }
      const nextTree = await loadPrivateTree()
      const createdNote = nextTree?.nodes.find((node): node is NoteNode => node.type === 'note' && node.id === created.id)
      setTitle('')
      setSlug('')
      setCreateOpen(false)
      if (createdNote) selectNote(createdNote)
    } catch (error) {
      setAdminMessage(error instanceof Error ? error.message : 'Creating the note failed.')
    } finally {
      setCreating(false)
    }
  }

  const draftDirty = draftNote ? editorText !== documentToText(draftNote.document) : false
  const tree = adminOpen ? privateTree : publicTree
  const treeError = adminOpen ? privateTreeError : publicTreeError

  return (
    <main className={sidebarOpen ? 'site-shell' : 'site-shell sidebar-collapsed'}>
      <aside className={sidebarOpen ? 'sidebar' : 'sidebar collapsed'} id="notes-sidebar">
        <div className="sidebar-header">
          {sidebarOpen ? <a className="brand" href="#">My Notes</a> : null}
          <button
            aria-controls="notes-sidebar-content"
            aria-expanded={sidebarOpen}
            aria-label={sidebarOpen ? 'Hide notes panel' : 'Show notes panel'}
            className="sidebar-toggle"
            onClick={() => setSidebarOpen((open) => !open)}
            title={sidebarOpen ? 'Hide notes panel' : 'Show notes panel'}
            type="button"
          >
            <span aria-hidden="true">{sidebarOpen ? '‹' : '›'}</span>
          </button>
        </div>
        <div className="sidebar-content" hidden={!sidebarOpen} id="notes-sidebar-content">
          <button
            className={adminOpen ? 'admin-toggle active' : 'admin-toggle'}
            onClick={() => {
              setAdminOpen((open) => !open)
              setAdminView('draft')
              setAdminMessage('')
            }}
          >
            {adminOpen ? 'Exit admin' : 'Admin'}
          </button>
          <p className="eyebrow">{adminOpen ? 'All pages' : 'Published pages'}</p>
          {adminOpen ? <>
            <button className="new-note-toggle" onClick={() => setCreateOpen((open) => !open)}>
              {createOpen ? 'Cancel' : '+ New note'}
            </button>
            {createOpen ? <form className="create-note" onSubmit={createNote}>
              <label>Title<input value={title} onChange={(event) => setTitle(event.target.value)} required /></label>
              <label>Slug<input value={slug} onChange={(event) => setSlug(event.target.value)} pattern="[a-z0-9]+(?:-[a-z0-9]+)*" required /></label>
              <button className="save-button" disabled={creating} type="submit">{creating ? 'Creating…' : 'Create note'}</button>
            </form> : null}
          </> : null}
          {treeError ? <p className="status error">{treeError}</p> : null}
          {!tree && !treeError ? <p className="status">Loading notes…</p> : null}
          {tree && notes.length === 0 ? <p className="status">{adminOpen ? 'No notes yet.' : 'No notes have been published yet.'}</p> : null}
          <nav aria-label={adminOpen ? 'All notes' : 'Published notes'}>
            {notes.map((candidate) => (
              <button
                className={candidate.id === selectedNote?.id ? 'note-link active' : 'note-link'}
                key={candidate.id}
                onClick={() => selectNote(candidate)}
              >
                <span>{candidate.title}</span>
                {adminOpen ? <small>{candidate.status}</small> : null}
              </button>
            ))}
          </nav>
        </div>
      </aside>
      <article className="note-page">
        {!selectedNote && tree ? <h1>Choose a note</h1> : null}
        {selectedNote ? <p className="eyebrow">{selectedNote.slug}{adminOpen ? ` · ${selectedNote.status}` : ''}</p> : null}
        {selectedNote ? <h1>{selectedNote.title}</h1> : null}
        {adminOpen && selectedNote ? <div className="view-switcher" aria-label="Page version">
          <button className={adminView === 'draft' ? 'active' : ''} onClick={() => setAdminView('draft')}>Draft</button>
          <button
            className={adminView === 'published' ? 'active' : ''}
            disabled={!selectedNote.publishedRevision}
            onClick={() => {
              setPublishedNote(null)
              setPublishedNoteError(null)
              setAdminView('published')
            }}
          >Published</button>
        </div> : null}

        {adminOpen && adminView === 'draft' && selectedNote ? <>
          {draftNote ? <>
            <textarea className="note-editor" aria-label="Note content" value={editorText} onChange={(event) => setEditorText(event.target.value)} />
            <div className="draft-actions">
              <button className="save-button" disabled={saving || !draftDirty} onClick={saveDraft}>{saving ? 'Saving…' : 'Save draft'}</button>
              <button className="publish-button" disabled={publishing || saving || draftDirty} onClick={publish}>
                {publishing ? 'Publishing…' : draftDirty ? 'Save before publishing' : selectedNote.status === 'published' ? 'Publish changes' : 'Publish page'}
              </button>
            </div>
            <section className="prose preview">{textToDocument(editorText).content?.map(renderBlock)}</section>
          </> : null}
          {adminMessage ? <p className={adminMessage.includes('failed') || adminMessage.includes('unavailable') ? 'status error' : 'status'}>{adminMessage}</p> : null}
        </> : null}

        {showPublished && selectedNote && !selectedPublishedRevision ? <p className="status">This page has not been published yet.</p> : null}
        {showPublished && selectedPublishedRevision && !publishedNote && !publishedNoteError ? <p className="status">Loading page…</p> : null}
        {showPublished && publishedNoteError ? <p className="status error">{publishedNoteError}</p> : null}
        {showPublished && publishedNote ? <section className="prose">{publishedNote.document.content?.map(renderBlock)}</section> : null}
      </article>
    </main>
  )
}

export default App
