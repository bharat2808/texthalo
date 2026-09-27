export type ContentBlock =
  | { type: "paragraph"; text: string }
  | { type: "heading"; text: string }
  | { type: "list"; items: string[] };

export type EditorialPage = {
  slug: string;
  title: string;
  description: string;
  datePublished: string;
  dateModified?: string;
  category: string;
  readTime: string;
  blocks: ContentBlock[];
};

export const blogPosts: EditorialPage[] = [
  {
    slug: "how-to-read-text-aloud-on-mac",
    title: "How to have your Mac read selected text aloud",
    description: "A practical guide to listening to selected text on macOS with TextHalo, from choosing a voice to starting and stopping playback.",
    datePublished: "2026-09-27",
    category: "How to",
    readTime: "3 min read",
    blocks: [
      { type: "paragraph", text: "Sometimes it is easier to listen to a passage than to keep reading it on screen. TextHalo is a macOS menu bar app that reads selected text aloud, so you can listen to an article, a draft, or a long email without moving your work into another app." },
      { type: "paragraph", text: "This guide covers the basic setup and reading loop. It assumes TextHalo is installed and running in your Mac’s menu bar. If you have not installed it yet, download the current macOS release from the TextHalo website." },
      { type: "heading", text: "Choose a voice" },
      { type: "paragraph", text: "Open TextHalo settings and choose an engine in Voice. Apple system voices are available immediately. Kokoro and Chatterbox are local AI speech engines; their model files need to be downloaded before you use them." },
      { type: "paragraph", text: "Starting with an Apple voice is the quickest way to confirm that the app is working. After that, you can preview available voices and decide whether you want to download an AI model. Model downloads take storage space and require an internet connection, while synthesis runs on the Mac once the model is ready." },
      { type: "heading", text: "Select text and start listening" },
      { type: "list", items: [
        "Select a passage in the app you are already using.",
        "Press TextHalo’s default shortcut, Command–Shift–S, to start reading.",
        "Use the floating player to follow playback or stop when you are done. Command–Shift–X is the default stop shortcut.",
      ] },
      { type: "paragraph", text: "If TextHalo cannot capture a selection in an app, open Settings → Capture and review the available capture methods. Accessibility-based capture requires permission in macOS System Settings → Privacy & Security → Accessibility. Copy-based capture is available for apps that do not expose selected text through Accessibility." },
      { type: "paragraph", text: "The Accessibility route lets the app ask the frontmost app for its selected text. Some apps do not expose selection text consistently; for those cases, the copy-based option can be a practical fallback. Choose the capture setting that works with the apps you use." },
      { type: "heading", text: "Keep the workflow comfortable" },
      { type: "paragraph", text: "Try a shorter passage first, then adjust the voice and speaking rate to suit the material. You can change the shortcuts in Settings → Shortcuts. TextHalo is designed to keep playback controls close while you continue working on your Mac." },
      { type: "paragraph", text: "For a long document, select one section at a time. That keeps listening sessions easy to pause and makes it simple to return to the original paragraph if you want to reread a sentence. To stop speech, use the floating player or press the stop shortcut." },
      { type: "paragraph", text: "Text-to-speech can make reading more flexible, but it is not a replacement for every assistive technology. If you rely on a screen reader or another accessibility tool, keep using the setup that meets your needs and check how TextHalo behaves alongside it." },
    ],
  },
  {
    slug: "local-text-to-speech-on-mac",
    title: "What local text-to-speech on Mac means for your privacy",
    description: "Learn what happens to selected text and downloaded voice models when you use TextHalo’s on-device speech engines on a Mac.",
    datePublished: "2026-09-27",
    category: "Privacy",
    readTime: "3 min read",
    blocks: [
      { type: "paragraph", text: "Text-to-speech tools do not all process text in the same place. Some send text to a remote service for synthesis. TextHalo’s Apple, Kokoro, and Chatterbox engines synthesize speech locally on your Mac." },
      { type: "paragraph", text: "Understanding the difference between synthesis and downloads helps describe privacy more precisely. A local speech engine does not mean the app never uses the internet; it means the speech is generated on the device instead of by a remote speech API." },
      { type: "heading", text: "Your selected text stays on your Mac for speech synthesis" },
      { type: "paragraph", text: "TextHalo captures the passage you select and sends it to the chosen local speech engine. The selected text is not sent to a cloud speech service by TextHalo. Generated audio is played on your Mac, and completed speech can be kept in the app’s local audio history." },
      { type: "paragraph", text: "The selected passage is used to produce the audio you asked to hear. TextHalo is built around reading selected text from other Mac apps, rather than uploading a document to a hosted transcription or speech-generation service." },
      { type: "heading", text: "Model downloads are a separate step" },
      { type: "paragraph", text: "Apple system voices are provided by macOS. Kokoro and Chatterbox require model files, which TextHalo downloads when you choose to use those engines. Downloading model files uses the internet; the speech synthesis itself runs locally after setup." },
      { type: "paragraph", text: "The app also needs an internet connection to fetch application updates. Model files and app updates are separate from the selected text used during local synthesis. If you want to use an AI voice without a network connection, first make sure its model is fully downloaded." },
      { type: "heading", text: "You choose how text is captured" },
      { type: "paragraph", text: "macOS Accessibility permission lets TextHalo read selected text from other apps that expose it. You can also use copy-based capture for apps that need it. Review the Capture settings and choose the method that fits your workflow." },
      { type: "paragraph", text: "The permission is controlled by macOS. You can review or revoke it in System Settings at any time. Copy-based capture interacts with the clipboard; the app’s Capture settings describe the available behavior and clipboard restoration option." },
      { type: "paragraph", text: "Local processing describes where speech synthesis runs. It does not mean every part of your Mac is offline: model downloads and app updates need network access. For implementation details, see the project’s privacy notes in the source repository." },
    ],
  },
  {
    slug: "listen-to-your-draft-while-proofreading",
    title: "A simple way to proofread a draft by listening",
    description: "Use text-to-speech on your Mac to review a passage by ear and notice wording that can be easy to miss while reading silently.",
    datePublished: "2026-09-27",
    category: "Workflow",
    readTime: "2 min read",
    blocks: [
      { type: "paragraph", text: "A sentence can look fine on screen and still sound awkward aloud. Listening to a draft gives you another way to review rhythm, repeated words, and transitions. It is a useful second pass, alongside careful editing." },
      { type: "paragraph", text: "This technique works for a short email, a paragraph in a report, or a section of a longer draft. The point is to change the way you review the words, not to replace your normal editing process." },
      { type: "heading", text: "Listen to one section at a time" },
      { type: "paragraph", text: "Select a paragraph or a few sentences in your writing app, then start TextHalo with Command–Shift–S. Working in short sections makes it easier to pause, make a change, and listen again without losing your place." },
      { type: "heading", text: "Listen for specific things" },
      { type: "list", items: [
        "Words or phrases that repeat close together.",
        "Sentences that run longer than you intended.",
        "Transitions that feel abrupt when spoken.",
        "Punctuation that changes the meaning or pacing of a sentence.",
      ] },
      { type: "paragraph", text: "Speech playback is a review aid, not a substitute for proofreading. Names, technical terms, and formatting still deserve a visual check. You can use an Apple voice right away or download a local AI voice engine in TextHalo settings." },
      { type: "paragraph", text: "If a sentence catches your ear, pause the player and revise it in the original app. Then select the updated passage and listen once more. Because TextHalo reads the current selection, the draft remains in the editor where you can make the change." },
    ],
  },
];

export const stories: EditorialPage[] = [
  {
    slug: "make-room-for-long-reading",
    title: "Make room for a long read",
    description: "A reading workflow for listening to a selected passage while taking a screen break, using TextHalo on macOS.",
    datePublished: "2026-09-27",
    category: "Reading workflow",
    readTime: "2 min read",
    blocks: [
      { type: "paragraph", text: "You have a long article open between meetings and want to get through a section without spending more time fixed on the screen." },
      { type: "paragraph", text: "You do not need to copy the whole article into a separate service. Start with a passage that fits the time you have, such as the introduction or one section with the context you need." },
      { type: "heading", text: "Keep the article where it is" },
      { type: "paragraph", text: "With TextHalo, you can leave the article in its browser tab. Select the section you want to hear, use the read-selection shortcut, and let the floating player stay nearby while you listen." },
      { type: "heading", text: "Take the passage at your pace" },
      { type: "paragraph", text: "Pause when you want to look back at a chart or reread a sentence. When you are ready, select the next section. The workflow stays anchored to the text and app you were already using." },
      { type: "paragraph", text: "This can also help when you are comparing a few passages. Listen to one, pause to take a note, then select another. The app’s menu bar controls and floating player keep the basic playback actions close at hand." },
      { type: "paragraph", text: "TextHalo runs speech synthesis locally on your Mac. Choose an Apple voice to begin, or download Kokoro or Chatterbox if you want to try a local AI voice." },
    ],
  },
  {
    slug: "hear-the-draft-another-way",
    title: "Hear a draft another way",
    description: "A practical writing workflow: listen to a selected passage on your Mac to review pace, repetition, and sentence flow.",
    datePublished: "2026-09-27",
    category: "Writing workflow",
    readTime: "2 min read",
    blocks: [
      { type: "paragraph", text: "You have finished a first pass and want a different perspective before editing. Instead of moving the text to another tool, select a passage in your editor and listen to it with TextHalo." },
      { type: "paragraph", text: "The writer starts with a paragraph rather than the entire document. A short selection makes it easier to notice a single issue and connect it to the exact wording on the page." },
      { type: "heading", text: "Listen for the shape of the sentence" },
      { type: "paragraph", text: "As the passage plays, the writer notices where the pace slows, where a phrase is repeated, and where a transition does not quite connect. They pause, revise the original text, then listen again." },
      { type: "paragraph", text: "For a second pass, they listen for one thing at a time: first repeated words, then long sentences, then whether the transition into the next paragraph feels natural. Focusing on one editing question can make the listening pass more useful." },
      { type: "heading", text: "Keep authorship with the writer" },
      { type: "paragraph", text: "TextHalo reads the selected words aloud; it does not rewrite or judge the draft. The writer decides what to change. Apple system voices work immediately, while optional Kokoro and Chatterbox engines run locally after their models are downloaded." },
      { type: "paragraph", text: "Listening is one more way to review writing. It works best alongside the usual visual checks for spelling, names, links, and formatting." },
    ],
  },
];
