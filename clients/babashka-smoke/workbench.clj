;; Separate wire example required by M17.1; no Runtime mutation/recovery logic.
(require '[cheshire.core :as json])
(import '[java.net Socket InetSocketAddress]
        '[java.io BufferedReader InputStreamReader OutputStreamWriter])

(defn check [condition message]
  (when-not condition (throw (ex-info message {}))))

(defn presentation-smoke [request result receive-until changed id baseline]
  (let [calls (atom 0)
        call (fn [op args] (request (str "declarative-" (swap! calls inc)) op args))
        query (fn [op args]
                (let [call-id (str "query-" (swap! calls inc))
                      submitted (result (request call-id "lab_query" {:op op :args args}))
                      _ (check (= "submitted" (:state submitted)) "query not submitted")
                      event (receive-until #(and (= "lab_update" (:event %))
                                                 (= call-id (get-in % [:data :call_id]))))]
                  (check (and (= id (:workbench_id event))
                              (= (:command_id submitted) (get-in event [:data :command_id]))
                              (= op (get-in event [:data :op]))
                              (= "result" (get-in event [:data :kind]))
                              (= "result" (get-in event [:data :runtime :type])))
                         (str "mediated query failed: " event))
                  (get-in event [:data :runtime :result])))
        before (result (call "presentation_get" {}))
        ;; Follow the existing frozen discovery contract, without a Runtime socket.
        first-page (query "discover" {})
        records (loop [page first-page records [] pages 0]
                  (check (< pages 16) "discovery page budget")
                  (check (= (:revision first-page) (:revision page)) "discovery view changed")
                  (let [records (into records (:records page))]
                    (if (:complete page)
                      records
                      (do (check (:next_index page) "discovery omitted next index")
                          (recur (query "discovery_page" {:projection (:projection page)
                                                         :index (:next_index page)})
                                 records (inc pages))))))
        signals (->> records
                     (filter #(and (= "signal" (:kind %)) (= "measurement" (:signal_kind %))
                                   (= "float" (:value_type %)))))
        ;; virtual-demo also exposes a moving mean of the same plant. A transform
        ;; output is real data, but is not an independent second measurement source.
        transforms (set (map :instrument (filter #(= "transform" (:component_kind %)) records)))
        selected (vec (take 2 (remove #(contains? transforms (:instrument %)) signals)))
        _ (check (seq selected) "virtual fixture has no numeric measurement signal")
        sources (mapv #(assoc (:id %) :kind "signal") selected)
        ;; Deliberately absent, labelled as a probe, never a second real source.
        missing (first (for [parameter (map str (range 1 1025))
                             :let [identity (assoc (:id (first selected)) :parameter parameter)]
                             :when (not-any? #(and (= "signal" (:kind %)) (= identity (:id %))) records)]
                         (assoc identity :kind "signal")))
        _ (check missing "no absent identity within probe budget")
        traces (mapv (fn [index signal source]
                       {:id (str "trace-" index) :source source :visible true
                        :display_label (:name signal) :display_unit (get-in signal [:unit :symbol])
                        :style {:color (nth ["#40c080" "#6090f0"] index) :width 2.0}})
                     (range) selected sources)
        document {:format_version 1 :document_id "dgui-3"
                  :windows [{:id "virtual-window" :title "DGUI-3 virtual bench"
                             :tabs [{:id "live-tab" :title "Live signals"
                                     :panels [{:id "live-status" :title "Current observation"
                                               :kind {:kind "status" :source (first sources)}}
                                              {:id "live-plot" :title "Live plot panel"
                                               :kind {:kind "plot" :plot_id "virtual-plot"}}]}
                                    {:id "missing-tab" :title "Missing data"
                                     :panels [{:id "missing-status" :title "Absent source probe"
                                               :kind {:kind "status" :source missing}}]}]}]
                  :plots [{:id "virtual-plot" :title "Discovered virtual measurements"
                           :time_window_seconds 60.0 :axes {:y_min nil :y_max nil} :traces traces}]
                  :controls []}
        observations (mapv #(query "latest" {:signal (:id %)}) selected)
        _ (Thread/sleep 300)
        newer-observations (mapv #(query "latest" {:signal (:id %)}) selected)
        expected {:workbench_id id :revision (:presentation_revision before)}
        _ (reset! changed [])
        applied (result (call "ui_replace_document" {:expected expected :document document}))
        next-revision (str (inc (bigint (:revision expected))))
        _ (when (empty? @changed) (receive-until #(= "presentation_changed" (:event %))))
        after (result (call "presentation_get" {}))
        stale (call "ui_replace_document" {:expected expected :document document})
        final (result (call "presentation_get" {}))
        status (result (call "client_status" {}))]
    (check (every? #(and (= "good" (:quality %)) (= "available" (:status %))
                        (number? (:value %)) (string? (:observed_at_ns %)))
                   (concat observations newer-observations))
           "current numeric observations missing")
    (check (every? true? (map #(> (bigint (:observed_at_ns %2)) (bigint (:observed_at_ns %1)))
                             observations newer-observations)) "observations did not advance")
    (check (= next-revision (:presentation_revision applied) (:presentation_revision after)
              (:presentation_revision final)) "replacement did not advance exactly once")
    (check (= id (:workbench_id applied) (:workbench_id after) (:workbench_id final))
           "replacement identity changed")
    (check (= document (:document after) (:document final)) "exact document readback failed")
    (check (= [{:v 1 :type "event" :workbench_id id :event "presentation_changed"
                :data {:presentation_revision next-revision :operation "ui_replace_document"}}] @changed)
           "expected exactly one replacement event")
    (check (and (= "error" (:type stale)) (= "revision_conflict" (get-in stale [:error :code])))
           "stale replacement was not rejected")
    (check (= (select-keys baseline [:runtime_client :recovery])
              (select-keys status [:runtime_client :recovery]))
           "read-only configuration changed Runtime attachment/recovery or freshness")
    {:status "PASS" :surface "declarative-workbench" :workbench_id id
     :presentation_revision next-revision :presentation_changed_count (count @changed)
     :exact_document true :revision_conflict true :signals sources
     :observations newer-observations :observations_advancing true :absent_source missing
     :discovered_signals (mapv #(select-keys % [:id :name :instrument :unit]) signals)
     :excluded_transform_instruments (vec (sort transforms))
     :absent_source_note "Unresolved renderer probe; not an observed signal"
     :runtime_attachment (select-keys (get-in status [:runtime_client :hello]) [:boot_id :scope :next_seq])
     :runtime_queries ["discover" "discovery_page (if paginated)" "latest"]
     :runtime_mutations_sent 0 :document document}))

(defn verify-presentation [request result changed id path]
  (let [previous (json/parse-string (slurp path) true)
        snapshot (result (request "verify-document" "presentation_get" {}))
        status (result (request "verify-status" "client_status" {}))]
    (check (= "declarative-workbench" (:surface previous)) "not a declarative evidence file")
    (check (= id (:workbench_id previous) (:workbench_id snapshot)) "Workbench identity changed")
    (check (= (:document previous) (:document snapshot)) "navigation/inspection changed document")
    (check (= (:presentation_revision previous) (:presentation_revision snapshot)) "inspection changed revision")
    (check (empty? @changed) "inspection emitted a presentation event")
    (check (= (:runtime_attachment previous)
              (select-keys (get-in status [:runtime_client :hello]) [:boot_id :scope :next_seq]))
           "Runtime attachment changed during inspection")
    (check (= "fresh" (get-in status [:runtime_client :freshness])) "inspection lost Fresh observations")
    {:status "PASS" :surface "declarative-readback" :workbench_id id
     :presentation_revision (:presentation_revision snapshot) :exact_document true
     :runtime_attachment_unchanged true :presentation_changed_count 0}))

(defn legacy-smoke [request result receive-until changed id]
  (let [submitted (result (request "lab" "lab_query" {:op "reference" :args {:reference "1"}}))]
    (check (= "submitted" (:state submitted)) "query was not submitted")
    (let [event (receive-until #(and (= "lab_update" (:event %))
                                    (= "lab" (get-in % [:data :call_id]))))]
      (check (and (= id (:workbench_id event))
                  (= (:command_id submitted) (get-in event [:data :command_id]))
                  (= "result" (get-in event [:data :kind]))
                  (= "result" (get-in event [:data :runtime :type]))
                  (= "1" (get-in event [:data :runtime :result :reference])))
             "mediated query lost its Runtime result/correlation")))
  (let [before (result (request "before" "presentation_get" {}))
        revision (:presentation_revision before)
        expected {:workbench_id id :revision revision}
        plot {:id (str "bb-smoke-" revision) :title "Babashka smoke"
              :time_window_seconds 60.0 :axes {:y_min nil :y_max nil} :traces []}
        mutation (result (request "add" "ui_add_plot" {:expected expected :plot plot}))
        next-revision (str (inc (bigint revision)))
        conflict (request "stale" "ui_remove_plot" {:expected expected :plot_id (:id plot)})
        after (result (request "after" "presentation_get" {}))]
    (check (= next-revision (:presentation_revision mutation) (:presentation_revision after))
           "UI revision did not advance exactly once")
    (check (and (= "error" (:type conflict))
                (= "revision_conflict" (get-in conflict [:error :code]))) "stale revision was not rejected")
    (check (= (update (:document before) :plots conj plot) (:document after))
           "UI mutation/conflict did not preserve the exact document")
    (check (and (= id (:workbench_id (last @changed)))
                (= next-revision (get-in (last @changed) [:data :presentation_revision]))
                (= "ui_add_plot" (get-in (last @changed) [:data :operation])))
           "missing presentation_changed event")
    {:status "PASS" :surface "workbench" :workbench_id id
     :presentation_revision next-revision :revision_conflict true
     :lab_update true :presentation_changed true}))

(let [[port & extra] *command-line-args*
      declarative? (= ["--presentation"] extra)
      verify-path (when (and (= "--verify-presentation" (first extra)) (= 2 (count extra))) (second extra))
      deadline (+ (System/nanoTime) 12000000000)
      changed (atom [])]
  (check (and port (or (empty? extra) declarative? verify-path))
         "usage: bb workbench.clj PORT [--presentation | --verify-presentation EVIDENCE.json]")
  (with-open [socket (Socket.)]
    (.connect socket (InetSocketAddress. "127.0.0.1" (Integer/parseInt port)) 2000)
    (.setSoTimeout socket 2000)
    (with-open [reader (BufferedReader. (InputStreamReader. (.getInputStream socket) "UTF-8"))
                writer (OutputStreamWriter. (.getOutputStream socket) "UTF-8")]
      (letfn [(receive-until [predicate]
                (loop [remaining 64]
                  (check (and (pos? remaining) (< (System/nanoTime) deadline)) "Workbench response deadline/budget")
                  (let [line (.readLine reader)]
                    (check (some? line) "Workbench closed before response")
                    (let [frame (json/parse-string line true)]
                      (check (= 1 (:v frame)) "unexpected Workbench version")
                      (when (= "presentation_changed" (:event frame)) (swap! changed conj frame))
                      (if (predicate frame) frame (recur (dec remaining)))))))
              (request [id op args]
                (when (or declarative? verify-path)
                  (check (contains? #{"hello" "client_status" "presentation_get" "lab_query"
                                      "ui_replace_document"} op) "unexpected declarative operation"))
                (.write writer (str (json/generate-string
                                     {:v 1 :type "request" :call_id id :op op :args args}) "\n"))
                (.flush writer)
                (receive-until #(= id (:call_id %))))
              (result [frame]
                (check (= "result" (:type frame)) (str "unexpected Workbench response: " frame))
                (:result frame))]
        (let [hello (result (request "hello" "hello" {}))
              id (:workbench_id hello)]
          (check (= {:id "lab-runtime.workbench" :version 1} (:protocol hello)) "unexpected Workbench protocol")
          (when declarative?
            (check (some #{"ui_replace_document"} (:operations hello)) "replacement not advertised"))
          ;; Bounded readiness observation only; no lab retry or recovery/status polling.
          (let [baseline (loop [attempt 0]
                           (check (and (< attempt 100) (< (System/nanoTime) deadline))
                                  "Runtime client readiness deadline")
                           (let [status (result (request (str "status-" attempt) "client_status" {}))]
                             (check (= id (:workbench_id status)) "Workbench identity changed")
                             (if (and (= "ready" (get-in status [:runtime_client :connection]))
                                      (or (not declarative?)
                                          (= "fresh" (get-in status [:runtime_client :freshness]))))
                               status
                               (do (Thread/sleep 20) (recur (inc attempt))))))]
            (println (json/generate-string
                       (cond
                         verify-path (verify-presentation request result changed id verify-path)
                         declarative? (presentation-smoke request result receive-until changed id baseline)
                         :else (legacy-smoke request result receive-until changed id))))))))))
