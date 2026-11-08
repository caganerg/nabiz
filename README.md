# nabiz

Intel işlemcili Linux makineler için sade, sakin bir terminal monitörü. Dört
şey gösterir, başka bir şey göstermez:

- çekirdek başına CPU kullanımı
- her çekirdeğin sıcaklığı, kullanımının hemen yanında
- RAM kullanımı (sayfa önbelleği daha açık renkli ayrı bir parça olarak)
- swap kullanımı

Süreç listesi yoktur. Tek bakışta okunacak bir gösterge paneli olarak
tasarlandı, htop'un yerine geçmek için değil.

İki çekirdeği yoğun çalışan bir i5-8500T'nin metin çıktısı (renkler
görünmüyor):

```
╭ nabiz ───────────────────────────────────────────────────────────────────────────────────╮
│  Intel Core i5-8500T @ 2.10GHz                                            açık 5sa 42dk  │
│  CPU  %34    Paket 61°C                                                             ▃▄▃  │
╰──────────────────────────────────────────────────────────────────────────────────────────╯
╭ Çekirdekler ──────────────────────────────────────────────── 6 çekirdek · 6 iş parçacığı ╮
│  cpu0   ███████████████████████████████████████████████████████████████████ %100   61°C  │
│  cpu1   ▋··································································   %1   49°C  │
│  cpu2   ██▋································································   %4   50°C  │
│  cpu3   ███████████████████████████████████████████████████████████████████ %100   58°C  │
│  cpu4   █▍·································································   %2   47°C  │
│  cpu5   ···································································   %0   46°C  │
╰──────────────────────────────────────────────────────────────────────────────────────────╯
╭ Bellek ──────────────────────────────────────────────────────── █ kullanılan  █ önbellek ╮
│  RAM   ███████████████████████████▍███████████████████████▎    8,1 GiB / 15,3 GiB   %53  │
│  Swap  ····················································    0,0 MiB / 12,3 GiB    %0  │
╰──────────────────────────────────────────────────────────────────────────────────────────╯

  q çıkış    +/- aralık                                                         her 1 sn  
```

Hyper-Threading açıksa kardeş thread'ler köşeli bir parantezle çift olarak
çizilir. Hibrit işlemcilerde (Alder Lake ve sonrası) P-çekirdekler ve
E-çekirdekler ayrı gruplarda gösterilir. Aşağıdaki görüntü, testlerdeki 12700H
fikstürünün 120 sütunda, her çekirdek %60 yükteyken çizilmiş hâli:

```
╭ Çekirdekler ───────────────────────────────────────────────────────────────────── 6P + 8E çekirdek · 20 iş parçacığı ╮
│  P  Performans   6 çekirdek · 12 iş parçacığı                                                                        │
│  cpu0  ╭ █████████████████████··············  %60   50°C    cpu6  ╭ █████████████████████··············  %60   62°C  │
│  cpu1  ╰ █████████████████████··············  %60   50°C    cpu7  ╰ █████████████████████··············  %60   62°C  │
│  cpu2  ╭ █████████████████████··············  %60   54°C    cpu8  ╭ █████████████████████··············  %60   66°C  │
│  cpu3  ╰ █████████████████████··············  %60   54°C    cpu9  ╰ █████████████████████··············  %60   66°C  │
│  cpu4  ╭ █████████████████████··············  %60   58°C    cpu10 ╭ █████████████████████··············  %60   70°C  │
│  cpu5  ╰ █████████████████████··············  %60   58°C    cpu11 ╰ █████████████████████··············  %60   70°C  │
│                                                                                                                      │
│  E  Verimlilik   8 çekirdek · 8 iş parçacığı                                                                         │
│  cpu12   █████████████████████··············  %60   34°C    cpu16   █████████████████████··············  %60   38°C  │
│  cpu13   █████████████████████··············  %60   35°C    cpu17   █████████████████████··············  %60   39°C  │
│  cpu14   █████████████████████··············  %60   36°C    cpu18   █████████████████████··············  %60   40°C  │
│  cpu15   █████████████████████··············  %60   37°C    cpu19   █████████████████████··············  %60   41°C  │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

## Derleme ve çalıştırma

1.88 veya daha yeni bir kararlı (stable) Rust araç zinciri gerekir.

```sh
cargo build --release
./target/release/nabiz
```

Ya da `~/.cargo/bin` altına kurmak için:

```sh
cargo install --path .
```

Debian/Ubuntu paketi olarak derlemek için (`dpkg-dev` paketi gerekir):

```sh
./scripts/build-deb.sh
sudo apt install ./target/debian/nabiz_0.1.0-1_amd64.deb
```

Paketin istediği en düşük glibc sürümü, derlendiği sistemden gelir.
`dpkg-shlibdeps` bunu binary'den hesaplayıp `Depends` alanına yazar, böylece
apt uyumsuz bir sisteme kurulumu baştan reddeder.

Normal kullanıcıyla çalışır, root yetkisi hiçbir zaman gerekmez.

| Tuş                  | İşlev                                          |
|----------------------|------------------------------------------------|
| `q`, `Esc`, `Ctrl-C` | çıkış                                          |
| `+` (veya `=`)       | yenileme aralığını uzat                        |
| `-`                  | yenileme aralığını kısalt                      |

Aralık şu adımlarla değişir: 250 ms, 500 ms, 750 ms, 1 sn (varsayılan), 1,5 sn,
2 sn, 3 sn, 4 sn ve 5 sn. Geçerli değer alt satırda gösterilir.

### Seçenekler ve ortam değişkenleri

- `nabiz --once` tek bir düz metin anlık görüntüsünü stdout'a yazıp çıkar.
  Arada 0,5 sn olan iki örnek alır, yani kullanım değerleri gerçek ölçümdür.
  Betiklerde kullanmak ve veri katmanının ne gördüğünü kontrol etmek için
  işe yarar.
- `NO_COLOR=1 nabiz` bütün renkleri kapatır. Çubuklar ve metin yerinde kalır;
  önbellek parçası kullanılan bellekten ayırt edilebilsin diye `░` ile çizilir.
- `COLORTERM=truecolor` (veya `24bit`) 24 bit rengi açar. Bu değişken yoksa
  renkler en yakın xterm-256 rengine eşlenir.

## Testler

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Bütün testler bellek içi bir dosya sisteminde tutulan metin fikstürleri
üzerinde çalışır. Gerçek `/proc` veya `/sys` hiç okunmaz; bu yüzden testler
root olmadan, Intel donanımı olmadan ve container içinde de geçer. Fikstürler
makinelerin tamamını tarif eder:

- HT'li dört çekirdekli bir i7
- hibrit bir i7-12700H (6P + 8E)
- hwmon'u olmayan ve bir CPU'sunun topoloji dosyaları eksik olan bir KVM
  sanal makinesi
- swap'ı olmayan bir dizüstü
- iki paketinde de `Core 0` bulunan çift soketli bir makine

Arayüz testleri ratatui'nin `TestBackend`'ine çizer ve şunları kontrol eder:
yerleşim, sütunlara dağılım, HT eşleştirmesi, taşma notu, "Terminal çok küçük"
ekranı ve `NO_COLOR` açıkken hiçbir hücrenin renklenmemesi.

## Tasarım notları

### Veri kaynakları

Bütün değerler düz metin dosyalarından `std::fs` ile okunur.

| Modül         | Kaynak                                    | Notlar |
|---------------|-------------------------------------------|--------|
| `cpu.rs`      | `/proc/stat`, `/proc/cpuinfo`             | meşgul = user+nice+system+irq+softirq+steal, boşta = idle+iowait; kullanım = Δmeşgul / (Δmeşgul+Δboşta). CPU'lar numaralarıyla tutulur, bu yüzden bir CPU'nun çevrimdışı olması diğerlerini kaydırmaz. |
| `temp.rs`     | `/sys/class/hwmon/hwmon*/`                | `coretemp`'i hwmon numarasıyla değil, `name` dosyasıyla bulur. `Package id X` ve `Core N` etiketlerini okur; eşikler `temp*_max` / `temp*_crit` dosyalarından gelir. |
| `topology.rs` | `/sys/devices/system/cpu/cpuN/topology/`, `/sys/devices/cpu_{core,atom}/cpus` | `core_id` ve `physical_package_id`; P/E tespiti; CPU aralık listesi ayrıştırıcısı (`0-3,8,10-11`). |
| `mem.rs`      | `/proc/meminfo`                           | kullanılan = MemTotal − MemAvailable; önbellek = Buffers + Cached + SReclaimable − Shmem; kullanılan swap = SwapTotal − SwapFree. |
| `collect.rs`  | `/proc/uptime`                            | Hepsini düz bir `Snapshot` yapısında birleştirir. |

`sysfs.rs` küçük `Fs` trait'ini içerir (dosya okuma, dizin listeleme).
Toplayıcılar dosyaları bunun üzerinden okur; testlerin bellek içi fikstür
kullanabilmesini sağlayan da budur.

### Mantıksal CPU'yu sıcaklığına eşlemek

`coretemp`, mantıksal CPU numarasına göre değil, `core_id` ile etiketlenmiş
olarak her fiziksel çekirdek için bir sıcaklık bildirir. Örneğin bir 12700H'de
cpu2 `Core 4`, E-çekirdek cpu19 ise `Core 31`'dir. Bu yüzden her mantıksal CPU,
`topology/core_id` ve `physical_package_id` üzerinden `(paket, çekirdek)`
anahtarına çözülür. Hyper-Threading kardeşleri aynı anahtarı, dolayısıyla aynı
okumayı paylaşır. Çok soketli makinelerde her coretemp örneğinin `Package id`
etiketi, çekirdeklerinin hangi pakete ait olduğunu söyler.

coretemp yoksa (AMD işlemci, sanal makine ya da modül yüklenmemiş) kullanım,
bellek ve swap yine çalışır; sıcaklıklar `—` olarak gösterilir.

### Renkler, çipin kendi sınırlarına göre

Sıcaklık rengi sabit sayılara bağlı değildir. Her okuma, sensörünün `high`
(`temp*_max`) ve `crit` (TjMax, `temp*_crit`) değerlerini taşır. Ölçek,
TjMax'ın %40'ında serin başlar ve `high` değerinde sıcağa ulaşır. `crit` ve
üstündeki okumalar ayrıca kalın yazılır. Bu dosyalar yoksa TjMax için 100 °C,
`high` için TjMax − 10 kullanılır.

Kullanım çubukları yeşil → sarı → turuncu → kırmızı bir geçiş kullanır. Her
hücre çubuk üzerindeki konumuna göre renklenir; böylece dolu bir çubuk bütün
ölçeği gösterir, çoğunlukla boşta duran bir çekirdek ise sakin bir yeşilde
kalır.

Palet yumuşak tonlardadır (Nord benzeri); ekran sakin kalır, sayılar öne çıkar.

### Çubuklar ve yerleşim

- Çubuklar `▏▎▍▌▋▊▉█` sekizlik bloklarını kullanır, bu da hücrenin 1/8'i kadar
  hassasiyet verir. Üst üste binen çubuklarda (RAM kullanılan + önbellek), sınır
  hücresi arka plan olarak bir sonraki parçanın rengini alır; böylece kenar
  pürüzsüz kalır.
- Kartlar yalnızca içerikleri kadar uzundur. 16'dan az mantıksal CPU'su olan
  makinelerde gösterge paneli en fazla 120 sütun genişliğindedir ve ortalanır.
- Çekirdekler sütunlara önce yukarıdan aşağı, sonra soldan sağa dağılır. 16 veya
  daha fazla mantıksal CPU varsa en az iki sütun olur. Daha fazla sütun yalnızca
  satırlar yüksekliğe sığmadığında eklenir ve hiçbir sütun 34 hücreden dar
  olmaz. Sütun geçişleri her zaman fiziksel çekirdeklerin arasına düşer, yani
  bir HT çifti asla bölünmez.
- En geniş yerleşim bile bütün çekirdekleri gösteremiyorsa, görünenler kesintisiz
  bir aralık olur ve kaç tanesinin gizlendiğini söyleyen bir not çıkar. 50×12'nin
  altında, yerleşimin yerine "Terminal çok küçük" mesajı gösterilir.

### Çalışma zamanı davranışı

- Döngü bir tuşa, yeniden boyutlandırmaya ya da bir sonraki tike kadar
  `crossterm::event::poll` içinde bekler; meşgul bekleme yoktur. coretemp keşfi
  başlangıçta bir kez yapılır, her tikte yalnızca `temp*_input` yeniden okunur.
  Topoloji yalnızca çevrimiçi CPU kümesi değiştiğinde yeniden okunur. Ölçülen
  yük, varsayılan 1 sn aralıkta bir çekirdeğin yaklaşık %0,3'ü (10 sn'de 27 ms
  CPU zamanı), 250 ms'de ise yaklaşık %1'idir.
- İlk kare sahte bir %0 yerine "ölçülüyor…" yazar. İkinci örnek 250 ms sonra
  alınır, böylece gerçek sayılar neredeyse hemen gelir.
- Terminal üç durumda eski hâline getirilir (raw mod kapatılır, alternatif
  ekrandan çıkılır, imleç gösterilir): normal çıkışta ve `?` hatalarında bir
  drop guard ile, panikte ise bir panic hook ile. Hook, terminali panik mesajını
  yazdırmadan *önce* düzeltir, bu sayede mesaj görünür kalır.

### Güvenlik ve sağlamlık

- Crate kökünde `#![forbid(unsafe_code)]` vardır. FFI yoktur, kendi `libc`
  çağrılarımız da yoktur.
- Çalışma zamanı verisi üzerinde `unwrap()` / `expect()` yoktur. Eksik ya da
  bozuk bir dosya yalnızca ilgili alanı "okunamadı" ya da `—` yapar; programı
  asla durdurmaz. Örneğin okunamayan bir `/proc/meminfo` bellek kartında
  "okunamadı" gösterir, okunamayan tek bir sensör ise yalnızca o çekirdeğin
  sıcaklığını düşürür.

### Bağımlılıklar

Yalnızca `ratatui` ve `crossterm`, başka hiçbir şey. `ratatui`,
`default-features = false` ile ve yalnızca `crossterm_0_29` arka uç özelliğiyle
derlenir; bu da makroları, yerleşim önbelleğini ve fazladan widget'ları dışarıda
bırakır. `nabiz`'in doğrudan kullandığı crossterm 0.29 sürümüne sabitlenmiştir,
böylece yalnızca tek bir kopya derlenir.

## Sınırlamalar

- `SIGTERM` / `SIGHUP` süreci yukarıdaki temizlik yapılmadan sonlandırır; nabiz
  dışarıdan öldürülürse `reset` çalıştırın. Sinyalleri yakalamak `libc` ya da
  `signal-hook` gibi bir crate gerektirirdi; bunlar FFI'sız ve az bağımlılıklı
  olma kurallarının dışında kalıyor. Klavyeden çıkışlar ve panikler
  kapsanmıştır.
- Sıcaklıklar bilerek yalnızca Intel `coretemp`'ten okunur. Diğer hwmon
  sürücüleri (`k10temp`, `acpitz`, …) yok sayılır.

## Lisans

MIT. Ayrıntılar için [LICENSE](LICENSE) dosyasına bakın.
