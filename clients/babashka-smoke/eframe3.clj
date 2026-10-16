;; External virtual-furnace presentation experiment. Workbench API only.
;; No Runtime socket, instrument mutation, recovery/status replay or GUI actions.
(require '[cheshire.core :as json])
(import '[java.net Socket InetSocketAddress]
        '[java.io BufferedReader InputStreamReader OutputStreamWriter])

(defn check [ok message] (when-not ok (throw (ex-info message {}))))
(defn document [source stage]
  (let [updated? (contains? #{"updated" "boundary"} stage)
        status (fn [id title target] {:id id :title title :kind {:kind "status" :source target}})
        plot (fn [id title] {:id id :title title :kind {:kind "plot" :plot_id "temperature-plot"}})
        controls (mapv (fn [i] {:id (str "inert-" i) :label (str "Read-only reference " i)
                                :kind "set_reference" :target {:kind "reference" :reference "1"} :confirm true}) (range 6))]
    (cond-> {:format_version 1 :document_id "eframe-3-virtual-furnace"
     :windows [{:id "furnace-window" :title (if updated? "EFRAME-3 API updated furnace" "EFRAME-3 Virtual furnace")
                :tabs [{:id "live-tab" :title "Live furnace"
                        :panels [(status "temperature" "Furnace temperature" source)
                                 (plot "temperature-panel" "Temperature history")]}
                       {:id "diagnostic-tab" :title "Freshness and unavailable data"
                        :panels [(status "missing" "Absent source probe" (assoc source :parameter "eframe-3-absent"))
                                 (status "reference" "Runtime reference" {:kind "reference" :reference "1"})
                                 (plot "second-plot-panel" "Same stock Plot on another tab")]}
                       {:id "inert-tab" :title "Read-only controls"
                        :panels [{:id "inert-panel" :title "Six groups, all actions disabled"
                                  :kind {:kind "controls" :control_ids (mapv :id controls)}}]}]}
               {:id "second-window" :title "Second configured presentation window"
                :tabs [{:id "second-tab" :title "Independent navigation"
                        :panels [(status "second-scalar" "Same approved furnace observation" source)]}]}]
     :plots [{:id "temperature-plot" :title (if updated? "API: 20 second temperature view" "Virtual furnace cooling, no output commands")
              :time_window_seconds (if updated? 20.0 120.0)
              :axes {:y_min (if updated? 19.0 nil) :y_max (if updated? 40.0 nil)}
              :traces [{:id "furnace-trace" :source source :display_label "Furnace" :visible true
                        :style {:color (if updated? "#ff6060" "#6090f0") :width (if updated? 3.0 2.0)} :display_unit "°C"}
                       ;; Same source deliberately repeated to exercise unique stock legend names.
                       {:id "duplicate-trace" :source source :display_label "Furnace" :visible true
                        :style {:color "#40c080" :width 1.0} :display_unit "degC"}]}]
     :controls controls}
      (= stage "boundary")
      (update :windows into
              (mapv (fn [wi]
                      {:id (str "boundary-window-" wi) :title "Large document boundary view"
                       :tabs (mapv (fn [ti] {:id (str "boundary-tab-" wi "-" ti)
                                             :title (apply str (repeat 512 "T")) :panels []})
                                   (range 52))})
                    (range 30))))))

(let [[port stage output] *command-line-args*
      transcript (atom [])
      calls (atom 0)]
  (check (and output (contains? #{"initial" "updated" "boundary" "verify"} stage))
         "usage: bb eframe3.clj PORT initial|updated|boundary|verify NEW_RESULT.json")
  (check (not (.exists (java.io.File. output))) "evidence destination already exists")
  (with-open [socket (Socket.)]
    (.connect socket (InetSocketAddress. "127.0.0.1" (Integer/parseInt port)) 2000)
    (.setSoTimeout socket 3000)
    (with-open [reader (BufferedReader. (InputStreamReader. (.getInputStream socket) "UTF-8"))
                writer (OutputStreamWriter. (.getOutputStream socket) "UTF-8")]
      (letfn [(call [op args]
                (check (contains? #{"hello" "client_status" "presentation_get" "ui_replace_document"} op) "unexpected operation")
                (let [id (str "eframe-3-" (swap! calls inc))
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
