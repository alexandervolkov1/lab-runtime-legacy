;; CONTROL-1 external document configuration. Existing Workbench API only.
;; No Runtime socket or mutations; user confirmation happens only in the native GUI.
(require '[cheshire.core :as json])
(import '[java.net Socket InetSocketAddress]
        '[java.io BufferedReader InputStreamReader OutputStreamWriter])

(defn check [ok message] (when-not ok (throw (ex-info message {}))))
(defn document [source stage]
  {:format_version 1 :document_id "control-1-virtual-furnace"
   :windows [{:id "window" :title (if (= stage "replace") "CONTROL-1 API replacement" "CONTROL-1 Virtual reference")
              :tabs [{:id "live" :title "Reference and live furnace"
                      :panels [{:id "reference-panel" :title "Runtime Reference and explicit retune"
                                :kind {:kind "controls" :control_ids ["retune"]}}
                               {:id "plot-panel" :title "Live virtual furnace"
                                :kind {:kind "plot" :plot_id "temperature"}}]}]}]
   :plots [{:id "temperature" :title "Virtual furnace measurement"
            :time_window_seconds 60.0 :axes {:time_mode "runtime_elapsed" :y_min nil :y_max nil}
            :traces [{:id "sample" :source source :display_label "Temperature" :visible true
                      :style {:color "#6090f0" :width 2.0} :display_unit "\u00b0C"}]}]
   :controls [{:id "retune" :label "Set Reference (ramp target/rate)" :kind "set_reference"
               :target {:kind "reference" :reference "1"} :confirm true}]})

(let [[port stage output] *command-line-args*
      transcript (atom [])
      calls (atom 0)]
  (check (and output (contains? #{"configure" "replace" "verify"} stage))
         "usage: bb control1.clj PORT configure|replace|verify NEW_RESULT.json")
  (check (not (.exists (java.io.File. output))) "evidence destination already exists")
  (with-open [socket (Socket.)]
    (.connect socket (InetSocketAddress. "127.0.0.1" (Integer/parseInt port)) 2000)
    (.setSoTimeout socket 3000)
    (with-open [reader (BufferedReader. (InputStreamReader. (.getInputStream socket) "UTF-8"))
                writer (OutputStreamWriter. (.getOutputStream socket) "UTF-8")]
      (letfn [(call [op args]
                (check (contains? #{"hello" "client_status" "presentation_get" "ui_replace_document"} op) "unexpected operation")
                (let [id (str "control-1-" (swap! calls inc))
                      request {:v 1 :type "request" :call_id id :op op :args args}]
                  (swap! transcript conj {:direction "request" :message request})
                  (.write writer (str (json/generate-string request) "\n")) (.flush writer)
                  (loop [remaining 64]
                    (check (pos? remaining) "response frame budget")
                    (let [line (.readLine reader)]
                      (check line "Workbench closed")
                      (let [frame (json/parse-string line true)]
                        (swap! transcript conj {:direction "response" :message frame})
                        (if (= id (:call_id frame))
                          (do (check (= "result" (:type frame)) (str frame)) (:result frame))
                          (recur (dec remaining))))))))]
        (let [hello (call "hello" {})
              before (call "presentation_get" {})
              baseline (call "client_status" {})
              source (get-in before [:document :plots 0 :traces 0 :source])
              _ (check (= "fresh" (get-in baseline [:runtime_client :freshness])) "Parent has not rebuilt Fresh")
              _ (check (= "signal" (:kind source)) "Parent document has no signal source")
              candidate (when-not (= stage "verify") (document source stage))
              applied (when candidate (call "ui_replace_document"
                                        {:expected {:workbench_id (:workbench_id hello) :revision (:presentation_revision before)}
                                         :document candidate}))
              after (call "presentation_get" {})
              status (call "client_status" {})
              attachment #(select-keys (get-in % [:runtime_client :hello]) [:boot_id :scope :next_seq])]
          (when candidate
            (check (= candidate (:document after)) "exact document readback failed")
            (check (= (str (inc (bigint (:presentation_revision before)))) (:presentation_revision applied) (:presentation_revision after)) "revision mismatch"))
          (check (= (attachment baseline) (attachment status)) "Runtime attachment changed")
          (let [result {:passed true :stage stage :workbench_id (:workbench_id hello)
                        :presentation_revision (:presentation_revision after) :source source
                        :runtime_attachment (attachment status) :document (:document after)
                        :runtime_mutations_sent 0 :runtime_sockets 0 :transcript @transcript}]
            (spit output (json/generate-string result {:pretty true}) :encoding "UTF-8")
            (println (json/generate-string (dissoc result :transcript :document)))))))))
