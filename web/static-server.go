// HTTP server that sets the headers needed for SharedArrayBuffer.

package main

import (
	"encoding/base64"
	"flag"
	"io"
	"log"
	"net/http"
	"os"
)

func main() {
	addr := flag.String("addr", "localhost:8080", "address to listen on")
	dir := flag.String("dir", ".", "directory to serve")
	frame := flag.String("frame", "frame.png", "where to write frames posted by the page")
	flag.Parse()

	mux := http.NewServeMux()
	// The page posts here to report errors, which is the only way to see what
	// went wrong when the browser's console isn't at hand.
	mux.HandleFunc("/log", func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		log.Printf("page: %s", body)
		w.WriteHeader(http.StatusNoContent)
	})
	// The page posts a PNG of its window here when started with ?frames=1,
	// which is how a script can see what the program actually drew.
	mux.HandleFunc("/frame", func(w http.ResponseWriter, r *http.Request) {
		body, err := io.ReadAll(r.Body)
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		png, err := base64.StdEncoding.DecodeString(string(body))
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		if err := os.WriteFile(*frame, png, 0o644); err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		w.WriteHeader(http.StatusNoContent)
	})
	mux.Handle("/", http.FileServer(http.Dir(*dir)))
	handler := withHeaders(mux)

	log.Printf("serving %s at http://%s", *dir, *addr)
	log.Fatal(http.ListenAndServe(*addr, handler))
}

func withHeaders(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		h := w.Header()

		// Cross-origin isolation: required for SharedArrayBuffer in browsers.
		h.Set("Cross-Origin-Opener-Policy", "same-origin")
		h.Set("Cross-Origin-Embedder-Policy", "require-corp")

		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}

		next.ServeHTTP(w, r)
	})
}
