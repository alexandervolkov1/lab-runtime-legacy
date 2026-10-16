;; External virtual-furnace presentation experiment. Workbench API only.
;; No Runtime socket, instrument mutation, recovery/status replay or GUI actions.
(require '[cheshire.core :as json])
(import '[java.net Socket InetSocketAddress]
        '[java.io BufferedReader InputStreamReader OutputStreamWriter])

(defn check [ok message] (when-not ok (throw (ex-info message {}))))
(defn document [source stage]
  (let [base {:format_version 1 :document_id "time-1"
              :controls []
              :windows [{:id "window" :title (str "TIME-1 " stage)
                         :tabs [{:id "tab" :title "Live virtual furnace"
                                 :panels [{:id "scalar" :title "Temperature"
                                           :kind {:kind "status" :source source}}
                                          {:id "plot-panel" :title "Measurement time"
                                           :kind {:kind "plot" :plot_id "plot"}}]}]}]
              :plots [{:id "plot" :title "Virtual furnace temperature"
                       :time_window_seconds 60.0 :axes {:y_min nil :y_max nil}
                       :traces [{:id "trace" :source source :display_label "Temperature"
                                 :visible true :style {:color "cyan" :width 2.0}
                                 :display_unit nil}]}]}]
    (if (= stage "default") base
        (assoc-in base [:plots 0 :axes :time_mode] stage))))

(let [[port stage output] *command-line-args*
      transcript (atom [])
      calls (atom 0)]
  (check (and output (contains? #{"default" "local_calendar" "utc" "runtime_elapsed" "experiment_elapsed" "verify"} stage))
         "usage: bb time1.clj PORT default|local_calendar|utc|runtime_elapsed|experiment_elapsed|verify NEW_RESULT.json")
  (check (not (.exists (java.io.File. output))) "evidence destination already exists")
  (with-open [socket (Socket.)]
    (.connect socket (InetSocketAddress. "127.0.0.1" (Integer/parseInt port)) 2000)
    (.setSoTimeout socket 3000)
    (with-open [reader (BufferedReader. (InputStreamReader. (.getInputStream socket) "UTF-8"))
                writer (OutputStreamWriter. (.getOutputStream socket) "UTF-8")]
      (letfn [(call [op args]
                (check (contains? #{"hello" "client_status" "presentation_get" "ui_replace_document"} op) "unexpected operation")
                (let [id (str "time-1-" (swap! calls inc))
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
