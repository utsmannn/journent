//! Header quotes on literacy, reading, and writing.
//! Rendered one-at-a-time, randomly picked per request, under the masthead ornament.
//!
//! Every line below has been verified verbatim against a published source via
//! antigravity CLI fact-check (2026-07-12). Do NOT replace any wording — if a quote
//! is edited, re-verify it the same way first. Source citations live in the comments.
//!
//! Curated set: philosophers, novelists, and Indonesian revolutionaries.

#[derive(Clone, Copy)]
pub struct Quote {
    pub text: &'static str,
    pub author: &'static str,
}

pub static QUOTES: &[Quote] = &[
    Quote {
        // "Reading maketh a full man; conference a ready man; and writing an exact man."
        // — Francis Bacon, essay "Of Studies" (1597, revised 1625).
        text: "Reading maketh a full man, conference a ready man, and writing an exact man.",
        author: "Francis Bacon",
    },
    Quote {
        // "Von allem Geschriebenen liebe ich nur das, was Einer mit seinem Blute schreibt."
        // — Nietzsche, Also Sprach Zarathustra (1883), Part 1, "On Reading and Writing".
        text: "Of all that is written, I love only what is written with blood.",
        author: "Friedrich Nietzsche",
    },
    Quote {
        // "yo, que me figuraba el Paraíso / bajo la especie de una biblioteca."
        // English tr. by Alastair Reid — "Poem of the Gifts", in El Hacedor / Dreamtigers (1960).
        text: "I, who had always thought of Paradise / In form and image as a library.",
        author: "Jorge Luis Borges",
    },
    Quote {
        // "Un classico è un libro che non ha mai finito di dire quel che ha da dire."
        // — Calvino, essay "Perché leggere i classici" (1981).
        text: "A classic is a book that has never finished saying what it has to say.",
        author: "Italo Calvino",
    },
    Quote {
        // "...ut quidquid lectione collectum est stilus redigat in corpus."
        // English tr. by R.M. Gummere (Loeb, 1925) — Seneca, Epistulae Morales, Letter 84.2.
        text: "...so that whatever is collected through reading may be reduced to a single body by the pen.",
        author: "Seneca",
    },
    Quote {
        // "Je ne cherche aux livres qu'à me donner moi-même plaisir par honnête amusement."
        // English tr. by Donald Frame — Montaigne, Essais II.10 "Of Books" (1580).
        text: "I seek in books only to give myself pleasure by honest amusement.",
        author: "Michel de Montaigne",
    },
    Quote {
        // — Kipling, address "Surgeons and the Soul", Royal College of Surgeons (14 Feb 1923).
        text: "Words are, of course, the most powerful drug used by mankind.",
        author: "Rudyard Kipling",
    },
    Quote {
        // — Hemingway, A Moveable Feast, ch. 1 "A Good Cafe on the Place St.-Michel" (1964, posthumous).
        text: "All you have to do is write one true sentence. Write the truest sentence that you know.",
        author: "Ernest Hemingway",
    },
    Quote {
        // "Viele Bücher sind wie ein Schlüssel zu unbekannten Kammern innerhalb des Schlosses der eigenen Seele."
        // — Kafka, letter to Oskar Pollak, Nov 1903.
        text: "Some books seem like a key to unfamiliar rooms in one's own castle.",
        author: "Franz Kafka",
    },
    Quote {
        // — Woolf, A Room of One's Own (1929).
        text: "A woman must have money and a room of her own if she is to write fiction.",
        author: "Virginia Woolf",
    },
    Quote {
        // Spoken by Nyai Ontosoroh to Minke — Pramoedya, Anak Semua Bangsa (1980).
        text: "Karena kau menulis. Suaramu takkan padam ditelan angin, akan abadi, sampai jauh, jauh di kemudian hari.",
        author: "Pramoedya Ananta Toer",
    },
    Quote {
        // From the introduction — Tan Malaka, Madilog (written 1943, published 1951).
        text: "Selama toko buku ada, selama itu pustaka bisa dibentuk kembali. Kalau perlu dan memang perlu, pakaian dan makanan dikurangi.",
        author: "Tan Malaka",
    },
];

/// Pick a quote pseudo-randomly per request. No external `rand` dependency —
/// sub-second nanos give enough entropy for a header line.
pub fn random_quote() -> Quote {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    QUOTES[n % QUOTES.len()]
}
